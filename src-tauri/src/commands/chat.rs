//! 对话命令（§6.1）+ 故障转移循环（§4.3）
//!
//! - chat：一次性返回完整结果（单供应商）
//! - chat_stream：立即返回；增量经 `chat:chunk` 推送，故障转移经 `chat:failover` 通知
//! - chat_abort：按 request_id 置位中断旗标
//!
//! 故障转移规则：
//! - 仅在「尚未输出任何内容」时转移，避免重复内容污染流式输出
//! - 候选 = 已启用、非当前、且有启用模型的供应商；按 strategy 排序
//! - 每次转移前指数退避；全部候选耗尽后回传最后一次错误

use crate::models::{
    ChatChunk, ChatRequest, ChatResponse, FailoverEvent, Model, Provider, ProviderType,
};
use crate::services::{failover, keyring, llm, llm::DeltaSink, llm::LlmContext, settings};
use crate::state::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State, Window};

const CHUNK_EVENT: &str = "chat:chunk";
const FAILOVER_EVENT: &str = "chat:failover";

/// 组装调用上下文：读供应商配置 + 全局代理 + keyring 取 Key + 建中断旗标
fn build_context(state: &AppState, provider_id: &str) -> Result<LlmContext, String> {
    let (provider, global_proxy) = {
        let conn = state.db()?;
        (
            super::providers::load_provider(&conn, provider_id)?,
            settings::get_global_proxy(&conn),
        )
    };
    let api_key = keyring::get_api_key(provider_id);

    // Ollama 本地服务通常无需 Key；其余供应商缺 Key 直接快速失败，避免发出必然 401 的请求
    if provider.provider_type != ProviderType::Ollama && api_key.is_none() {
        return Err(format!(
            "UNAUTHORIZED:供应商「{}」未配置 API Key",
            provider.name
        ));
    }

    Ok(LlmContext {
        provider,
        api_key,
        abort: Arc::new(AtomicBool::new(false)),
        global_proxy,
    })
}

#[tauri::command]
pub async fn chat(
    request: ChatRequest,
    state: State<'_, AppState>,
) -> Result<ChatResponse, String> {
    let ctx = build_context(&state, &request.provider_id)?;
    let noop: DeltaSink = Box::new(|_, _| {});
    llm::execute(ctx, request, &noop).await
}

#[tauri::command]
pub async fn chat_stream(
    request: ChatRequest,
    state: State<'_, AppState>,
    window: Window,
    app: AppHandle,
) -> Result<(), String> {
    let ctx = build_context(&state, &request.provider_id)?;
    let request_id = request.request_id.clone();
    state.register_active(&request_id, ctx.abort.clone());

    // 故障转移所需的快照在入任务前读取，避免异步任务内持锁
    let (failover_cfg, all_providers, all_models) = {
        let conn = state.db()?;
        (
            settings::get_failover_config(&conn),
            super::providers::load_providers(&conn)?,
            super::models::load_models(&conn, None)?,
        )
    };

    let abort_flag = ctx.abort.clone();
    let session_id = request.session_id.clone();
    let global_proxy = ctx.global_proxy.clone();
    let initial_provider = ctx.provider.clone();
    let initial_key = ctx.api_key.clone();

    tauri::async_runtime::spawn(async move {
        let mut provider = initial_provider;
        let mut api_key = initial_key;
        let mut model_id = request.model_id.clone();
        let mut attempt: u32 = 0;
        let mut candidates: Vec<(Provider, Model)> = Vec::new();
        // 所有 break 路径均先赋值再退出，无需默认值
        let result: Result<ChatResponse, String>;

        loop {
            // 每次尝试独立 sink：记录是否已产出内容（产出后不再转移，避免内容重复）
            let emitted = Arc::new(AtomicBool::new(false));
            let emitted_flag = emitted.clone();
            let rid = request_id.clone();
            let win2 = window.clone();
            // delta 聚合（性能）：逐 token emit 会造成 IPC 事件风暴，
            // 这里按 16ms 窗口/8KB 阈值批量合并后再推给前端；首包立即发出降低首字延迟
            let rid_for_flush = request_id.clone();
            // (content 缓冲, reasoning 缓冲, 上次 flush 时刻)
            let agg = Arc::new(std::sync::Mutex::new((
                String::new(),
                String::new(),
                None::<std::time::Instant>,
            )));
            let agg2 = agg.clone();
            let sink: DeltaSink = Box::new(move |delta: &str, reasoning: Option<&str>| {
                if !delta.is_empty() || reasoning.is_some() {
                    emitted_flag.store(true, Ordering::Relaxed);
                }
                let mut guard = agg2.lock().unwrap();
                let first_flush = guard.2.is_none();
                guard.0.push_str(delta);
                if let Some(r) = reasoning {
                    guard.1.push_str(r);
                }
                let buffered = guard.0.len() + guard.1.len();
                let should_flush = first_flush
                    || buffered >= 8 * 1024
                    || guard
                        .2
                        .map(|t| t.elapsed().as_millis() >= 16)
                        .unwrap_or(false);
                if should_flush {
                    guard.2 = Some(std::time::Instant::now());
                    let chunk = ChatChunk {
                        request_id: rid.clone(),
                        delta: std::mem::take(&mut guard.0),
                        reasoning_delta: if guard.1.is_empty() {
                            None
                        } else {
                            Some(std::mem::take(&mut guard.1))
                        },
                        done: false,
                        result: None,
                        error: None,
                        error_message: None,
                    };
                    drop(guard);
                    let _ = win2.emit(CHUNK_EVENT, &chunk);
                }
            });

            let mut attempt_req = request.clone();
            attempt_req.model_id = model_id.clone();

            match crate::services::agent::run(
                &app,
                &app.state::<AppState>(),
                attempt_req,
                provider.clone(),
                api_key.clone(),
                global_proxy.clone(),
                abort_flag.clone(),
                window.clone(),
                &sink,
            )
            .await
            {
                Ok(resp) => {
                    // 冲掉聚合缓冲的残留尾巴，保证流式内容完整（失败转移路径同理）
                    {
                        let mut guard = agg.lock().unwrap();
                        if !guard.0.is_empty() || !guard.1.is_empty() {
                            let chunk = ChatChunk {
                                request_id: rid_for_flush.clone(),
                                delta: std::mem::take(&mut guard.0),
                                reasoning_delta: if guard.1.is_empty() {
                                    None
                                } else {
                                    Some(std::mem::take(&mut guard.1))
                                },
                                done: false,
                                result: None,
                                error: None,
                                error_message: None,
                            };
                            drop(guard);
                            let _ = window.emit(CHUNK_EVENT, &chunk);
                        }
                    }
                    result = Ok(resp);
                    break;
                }
                Err(e) => {
                    // 尝试失败前冲掉聚合缓冲，避免已生成内容随转移丢失
                    {
                        let mut guard = agg.lock().unwrap();
                        if !guard.0.is_empty() || !guard.1.is_empty() {
                            let chunk = ChatChunk {
                                request_id: rid_for_flush.clone(),
                                delta: std::mem::take(&mut guard.0),
                                reasoning_delta: if guard.1.is_empty() {
                                    None
                                } else {
                                    Some(std::mem::take(&mut guard.1))
                                },
                                done: false,
                                result: None,
                                error: None,
                                error_message: None,
                            };
                            drop(guard);
                            let _ = window.emit(CHUNK_EVENT, &chunk);
                        }
                    }
                    // 用户中断：立即退出，不做任何容灾
                    if abort_flag.load(Ordering::Relaxed) || e.starts_with("ABORTED") {
                        result = Err(e);
                        break;
                    }
                    let can_failover = failover::is_retryable(&e, &failover_cfg)
                        && !emitted.load(Ordering::Relaxed);
                    if !can_failover {
                        result = Err(e);
                        break;
                    }
                    // 首次失败时计算候选快照：
                    // 用户自选备选链优先（可跨/同供应商多模型，按用户排序），
                    // 未配置链时回退自动发现（旧行为：其他启用供应商各取首个模型）
                    if candidates.is_empty() {
                        candidates = if failover_cfg.fallback_chain.is_empty() {
                            failover::pick_candidates(
                                &all_providers,
                                &all_models,
                                &provider.id,
                                failover_cfg.strategy,
                            )
                        } else {
                            failover::pick_from_chain(
                                &failover_cfg.fallback_chain,
                                &all_providers,
                                &all_models,
                            )
                        };
                        candidates.retain(|(p, _)| {
                            p.provider_type == ProviderType::Ollama
                                || keyring::get_api_key(&p.id).is_some()
                        });
                    }
                    let idx = attempt as usize;
                    if idx >= candidates.len() {
                        result = Err(e);
                        break;
                    }
                    let (next_provider, next_model) = candidates[idx].clone();

                    // 故障日志落库 + 前端非侵入通知（§4.3）
                    // 注意：先绑定 Result 再匹配，避免 if-let 临时值跨 await 的借用问题
                    {
                        let st = app.state::<AppState>();
                        let conn_res = st.db();
                        if let Ok(conn) = conn_res {
                            failover::log_failover(
                                &conn,
                                session_id.as_deref(),
                                &provider.name,
                                &next_provider.name,
                                &e,
                            );
                        }
                    }
                    let _ = window.emit(
                        FAILOVER_EVENT,
                        &FailoverEvent {
                            request_id: request_id.clone(),
                            from_provider: provider.name.clone(),
                            to_provider: next_provider.name.clone(),
                            reason: failover::split_error(&e).0.to_string(),
                        },
                    );

                    // 指数退避后接管：delay = min(base * multiplier^attempt, maxBackoff)
                    tokio::time::sleep(failover::backoff_delay(attempt, &failover_cfg)).await;
                    if abort_flag.load(Ordering::Relaxed) {
                        result = Err(e);
                        break;
                    }

                    provider = next_provider;
                    api_key = keyring::get_api_key(&provider.id);
                    model_id = next_model.model_id.clone();
                    attempt += 1;
                }
            }
        }

        // 任务结束清理中断注册表，并发送终态事件
        let st = app.state::<AppState>();
        st.remove_active(&request_id);

        let final_chunk = match result {
            Ok(resp) => ChatChunk {
                request_id,
                delta: String::new(),
                reasoning_delta: None,
                done: true,
                result: Some(resp),
                error: None,
                error_message: None,
            },
            Err(e) => {
                let (code, msg) = failover::split_error(&e);
                ChatChunk {
                    request_id,
                    delta: String::new(),
                    reasoning_delta: None,
                    done: true,
                    result: None,
                    error: Some(code.to_string()),
                    error_message: Some(msg.to_string()),
                }
            }
        };
        let _ = window.emit(CHUNK_EVENT, &final_chunk);
    });

    Ok(())
}

#[tauri::command]
pub fn chat_abort(request_id: String, state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.abort(&request_id))
}
