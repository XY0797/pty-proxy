# 终端代理

将诸如Python等不支持`重定向IO`的应用，使用`ConPTY API`代理成可`重定向IO`的程序

比如：配置`pty-proxy`的目标应用为python，就可以把`pty-proxy`当成python直接启动然后`重定向IO`

## 名词解释

`重定向IO`：CreateProcess创建进程时提供STARTUPINFO指定hStdInput、hStdOutput、hStdError，以此获取控制台输出的方法叫重定向IO。

`ConPTY API`：虚拟终端API，使用CreatePseudoConsole创建虚拟终端，详见[微软的API文档](https://learn.microsoft.com/zh-cn/windows/console/creating-a-pseudoconsole-session)。

## 使用

一共有两个文件：

`pty-proxy.exe`：主程序，可随意重命名

`pty-proxy-child.exe`：辅助程序，名字不可改，必须和主程序在同一个目录下！

## 开发

先运行一次`debug模式`的构建：

```sh
cargo build --features debug_mode
```

构建过后才能运行：

```sh
cargo run --features debug_mode --bin pty-proxy -- cmd.exe /k echo Hello, World!
```

如果修改了`pty-proxy-child`，则需要重新运行一次`debug模式`的构建

## 发行

```sh
cargo build --release
```

`target/release`下的可执行文件即为构建产物