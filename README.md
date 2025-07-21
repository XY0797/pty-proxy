# 终端代理

将诸如Python等不支持`重定向IO`的应用，使用`ConPTY/WinPTY API`代理成可`重定向IO`的程序

比如：配置`pty-proxy`的目标应用为python，就可以把`pty-proxy`当成python直接启动然后`重定向IO`

## 名词解释

`重定向IO`：CreateProcess创建进程时提供STARTUPINFO指定hStdInput、hStdOutput、hStdError，以此获取控制台输出的方法叫重定向IO。

`ConPTY API`：虚拟终端API，使用CreatePseudoConsole创建虚拟终端，需要Win10以上的操作系统才可用。详见[微软的API文档](https://learn.microsoft.com/zh-cn/windows/console/creating-a-pseudoconsole-session)。

`WinPTY API`：一个社区维护的项目，Win7及以上皆可用。详见[项目主页](https://github.com/rprichard/winpty)。

## 使用

一共有两个文件：

`pty-proxy.exe`：主程序，可随意重命名

`pty-proxy-child.exe`：辅助程序，名字不可改，必须和主程序在同一个目录下！

终端内启动`pty-proxy.exe`即查看详细用法

注意：输出的内容含有[VT-100转义序列](https://learn.microsoft.com/zh-cn/windows/console/console-virtual-terminal-sequences)，又叫`ANSI转义序列`。需要处理掉这些转义序列才能得到正常的文本。推荐使用后端为`WinPTY`的版本，因为这个版本的转义序列会显著少于`ConPTY`后端的版本，使用正则`\x1B\[(.*?)[A-Za-z]`即可去除大部分转义序列。

## 开发

先运行一次`debug模式`的构建：

```sh
cargo build --features debug_mode --features conpty
```

构建过后才能运行：

```sh
cargo run --features debug_mode --features conpty --bin pty-proxy -- cmd.exe /k echo Hello, World!
```

如果修改了`pty-proxy-child`，则需要重新运行一次`debug模式`的构建

如果要开发`winpty`后端的版本，把`--features conpty`换成`--features winpty`即可。

## 发行

### ConPTY后端

```sh
cargo build --features conpty --release
```

`target/release`下的可执行文件即为构建产物

### WinPTY后端

构建前先去`https://github.com/rprichard/winpty`下载`release`，比如`winpty-0.4.3-msvc2015.zip`。

然后在合适位置创建一个`winpty_dev`文件夹，并且添加到环境变量`PATH`中

把`release`包内的`x64/bin/winpty.dll`、`x64/bin/winpty-agent.exe`、`x64/lib/winpty.lib`复制到`winpty_dev`文件夹内

最后使用以下命令构建：

```sh
cargo build --features winpty --release
```

`target/release`下的可执行文件即为构建产物，发行时需要把`winpty-agent.exe`、`winpty.dll`和构建产物放到同一个文件夹下，否则会无法运行。