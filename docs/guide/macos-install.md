# macOS 安装说明

在 macOS 上打开 Koid 时，可能会遇到"应用已损坏"或"无法验证开发者"的提示。这通常是 macOS 安全机制所致，而非应用本身损坏。

## 问题现象

打开应用时出现：

> "Koid" 已损坏，无法打开。您应该将它移到废纸篓。

或

> 无法打开 "Koid"，因为 Apple 无法检查其是否包含恶意软件。

## 原因

macOS Gatekeeper 安全机制：从非 Mac App Store 下载、且未经 Apple 签名公证的应用会被阻止运行。Koid 目前未做 Apple 开发者签名，因此触发此保护。

## 解决方案

### 方法一：移除隔离属性（推荐）

打开 **终端**，执行：

```bash
sudo xattr -r -d com.apple.quarantine /Applications/Koid.app
```

输入管理员密码后重新打开应用即可。

### 方法二：右键打开

1. 在 Finder 中找到 `Koid.app`
2. 按住 `Control` 键点击应用图标
3. 在弹出菜单中选择 **打开**
4. 在确认对话框中点击 **打开**

可能需要重复 2-3 次才能成功。

### 方法三：临时允许任意来源

::: warning 安全提示
此方法会降低系统安全性，仅建议临时使用，安装完成后建议恢复设置。
:::

```bash
# 允许任意来源
sudo spctl --master-disable
```

然后在 **系统偏好设置 → 安全性与隐私 → 通用** 中选择 **任何来源**，打开 Koid 后恢复：

```bash
sudo spctl --master-enable
```

## 验证完整性

下载后可校验 SHA256：

```bash
shasum -a 256 ~/Downloads/Koid_0.1.0_universal.dmg
```

与 GitHub Release 页面提供的校验和对比。

## 关于芯片架构

Koid 的 macOS 安装包是 **universal** 版本（`Koid_0.1.0_universal.dmg`），同时支持 Intel 与 Apple Silicon（M1/M2/M3/M4），**无需 Rosetta 2**，也无需区分架构下载。

## 仍然无法解决？

完全删除后重装：

```bash
rm -rf /Applications/Koid.app
rm -rf ~/Library/Application\ Support/studio.fishpond.koid
rm -rf ~/Library/Caches/studio.fishpond.koid
```

然后从 [GitHub Releases](https://github.com/fishpond-studio/Koid/releases) 重新下载，并用方法一移除隔离属性。
