# 终端代理

将诸如Python等不支持`重定向IO`的应用，使用`ConPTY API`代理成可`重定向IO`的程序

比如：配置`pty-proxy`的目标应用为python，就可以把`pty-proxy`当成python直接启动然后`重定向IO`

## 名词解释

`重定向IO`：CreateProcess创建进程时提供STARTUPINFO指定hStdInput、hStdOutput、hStdError，以此获取控制台输出的方法叫重定向IO。

`ConPTY API`：虚拟终端API，使用CreatePseudoConsole创建虚拟终端，详见[微软的API文档](https://learn.microsoft.com/zh-cn/windows/console/creating-a-pseudoconsole-session)。

## 使用

一共有三个文件：

`pty-proxy.exe`：主程序，可随意重命名

`pty-proxy-child.exe`：辅助程序，名字不可改

`pty-proxy-color.exe`：主程序，可随意重命名；与`pty-proxy.exe`不同的是，它会主动开启终端的VT100转义序列处理功能

## 开发

```sh
cargo run --features debug_mode --bin pty-proxy -- cmd.exe /k echo Hello, World!
```

```sh
cargo build --features debug_mode
```

## 构建

```sh
cargo build --release
```