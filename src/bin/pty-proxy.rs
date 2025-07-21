use std::ffi::{ OsStr, c_void };
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{ AsRawHandle, OwnedHandle, FromRawHandle };
use std::ptr::null_mut;
use std::fs;
use std::io::{ self, Read, Write };
use std::path::Path;
use std::process::exit;
use std::sync::{ Arc, Mutex };
use std::thread;
use std::mem::{ zeroed, size_of };

use uuid::Uuid;
use toml::Value;
use windows_sys::{
    Win32::Foundation::*,
    Win32::Storage::FileSystem::*,
    Win32::System::Pipes::*,
    Win32::System::Threading::*,
};

#[cfg(feature = "color")]
use windows_sys::Win32::System::Console::*;

macro_rules! debug_println {
    ($($arg:tt)*) => {
        #[cfg(feature = "debug_mode")]
        {
            println!($($arg)*);
        }
    };
}

fn to_wstr(s: &str) -> Vec<u16> {
    // 将 Rust 字符串转换为 OsStr
    let os_str = OsStr::new(s);
    // 将 OsStr 转换为 UTF-16 编码的宽字符数组
    let wide_chars: Vec<u16> = os_str.encode_wide().chain(Some(0).into_iter()).collect();
    // 返回宽字符数组
    wide_chars
}

/// 启动一个独立的进程
///
/// # 参数
/// - `command`: 要执行的命令（包括路径和参数）
///
/// # 返回值
/// - `Ok(进程句柄)` 如果成功
/// - `Err(错误信息)` 如果失败
fn create_independent_process(command: &String) -> Result<OwnedHandle, String> {
    debug_println!("启动独立进程，命令行：{}", command);
    let mut command_line = to_wstr(command.as_str());

    let mut startup_info: STARTUPINFOW = unsafe { zeroed() };
    startup_info.cb = size_of::<STARTUPINFOW>() as u32;
    startup_info.dwFlags = STARTF_USESHOWWINDOW;
    startup_info.wShowWindow = 0; // SW_HIDE

    #[cfg(feature = "debug_mode")]
    {
        startup_info.wShowWindow = 1; // SW_NORMAL
    }

    let mut process_info: PROCESS_INFORMATION = unsafe { zeroed() };

    let success = unsafe {
        CreateProcessW(
            null_mut(), // 使用命令行而不是应用程序名称
            command_line.as_mut_ptr(), // 命令行
            null_mut(), // 进程安全属性
            null_mut(), // 线程安全属性
            false as i32, // 不继承句柄
            CREATE_NEW_PROCESS_GROUP | CREATE_NEW_CONSOLE, // 标志
            null_mut(), // 使用父进程的环境
            null_mut(), // 使用父进程的工作目录
            &mut startup_info, // 启动信息
            &mut process_info // 进程信息
        )
    };

    if success == 0 {
        let error = io::Error::last_os_error();
        Err(format!("无法启动进程: {}", error))
    } else {
        unsafe {
            CloseHandle(process_info.hThread);
            Ok(OwnedHandle::from_raw_handle(process_info.hProcess as *mut c_void))
        }
    }
}

fn main() {
    #[cfg(feature = "debug_mode")]
    {
        use std::panic;
        panic::set_hook(
            Box::new(|panic_info| {
                println!("\n{}\n", panic_info);
                println!("Press enter to exit...");
                let mut buf = vec![0;1];
                use std::io::Read;
                io::stdin().lock().read_exact(&mut buf).unwrap();
            })
        );
    }
    // 设置控制台属性
    #[cfg(feature = "color")]
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut console_mode: CONSOLE_MODE = 0;
        GetConsoleMode(handle, &mut console_mode);
        console_mode |= ENABLE_VIRTUAL_TERMINAL_PROCESSING;
        SetConsoleMode(handle, console_mode);
    }
    // 获取当前可执行文件的文件名（不带扩展名）
    let exe_path = std::env::current_exe().expect("无法获取当前可执行文件路径");
    let exe_dir = exe_path.parent().expect("无法获取可执行文件目录");
    let exe_name = Path::new(&exe_path)
        .file_stem()
        .expect("无法获取可执行文件名")
        .to_str()
        .expect("无法将可执行文件名转换为字符串");
    let child_exe_path = exe_dir.join("pty-proxy-child.exe");
    let child_program: String = String::from(child_exe_path.to_str().expect("无法获取子程序路径"));

    // 构造配置文件名
    let config_file_name = format!("{}.toml", exe_name);
    let config_file_path = exe_dir.join(&config_file_name);

    debug_println!("路径信息:");
    debug_println!(
        "exe_path: {}",
        exe_path.as_path().to_str().expect("无法调试输出可执行文件路径")
    );
    debug_println!("self_exe_dir: {}", exe_dir.to_str().expect("无法调试输出可执行文件目录"));
    debug_println!("self_exe_name: {}", exe_name);
    debug_println!("child_program: {}", child_program);
    debug_println!();

    // 检查配置文件是否存在
    let (target_program, target_args) = if config_file_path.exists() {
        // 读取配置文件内容
        let config_content = fs::read_to_string(config_file_path).expect("无法读取配置文件");

        // 解析 TOML 配置文件
        let config: Value = config_content.parse().expect("无法解析配置文件");

        // 从配置中获取 target_program 和 args
        let target_program = config["target_program"]
            .as_str()
            .expect("配置文件中缺少target_program或内容无效");

        let target_args = config["args"]
            .as_array()
            .expect("配置文件中缺少args或内容无效")
            .iter()
            .map(|v| v.as_str().expect("配置文件中的参数无效"))
            .collect::<Vec<&str>>()
            .join(" "); // 将参数列表拼接成一个字符串

        (String::from(target_program), target_args)
    } else {
        // 解析命令行参数
        let args: Vec<String> = std::env::args().collect();
        if args.len() < 2 {
            eprintln!(
                concat!(
                    "用法: {} <target_program> [args...]\n\n",
                    "或者在 {}.toml 中编写配置，示例：\n",
                    "target_program = \"cmd.exe\"\n",
                    "args = [\"/C\", \"echo helloworld\"]"
                ),
                exe_name,
                exe_name
            );
            panic!("至少要1个命令行参数或编写配置文件才能运行！");
        }

        // 获取目标程序路径和参数
        let target_program = args[1].clone();
        let target_args = args[2..].join(" "); // 将参数列表拼接成一个字符串

        (target_program, target_args)
    };

    debug_println!("配置信息：");
    debug_println!("target_program: {}", target_program);
    debug_println!("target_args: {}", target_args);
    debug_println!();

    // 生成唯一的命名管道名称
    let pipe_uuid_read: String = format!("{}", Uuid::new_v4()).replace("-", "");
    let pipe_uuid_write: String = format!("{}", Uuid::new_v4()).replace("-", "");
    let pipe_name_read: String = format!(r"\\.\pipe\ptyproxy{}", pipe_uuid_read);
    let pipe_name_write: String = format!(r"\\.\pipe\ptyproxy{}", pipe_uuid_write);

    // 创建命名管道
    let pipe_handle_read: Arc<Mutex<OwnedHandle>> = Arc::new(
        Mutex::new(create_named_pipe_read(&pipe_name_read).expect("无法创建命名管道读端"))
    ); // 包装为线程安全
    let pipe_handle_write = Arc::new(
        Mutex::new(create_named_pipe_write(&pipe_name_write).expect("无法创建命名管道写端"))
    ); // 包装为线程安全

    // 连接命名管道
    let pipe_handle_connect_read = Arc::clone(&pipe_handle_read);
    let pipe_handle_connect_write = Arc::clone(&pipe_handle_write);
    let connect_pipe_thread_handle_read = thread::spawn(move || {
        connect_named_pipe(&pipe_handle_connect_read).expect("无法连接命名管道读端");
    });
    let connect_pipe_thread_handle_write = thread::spawn(move || {
        connect_named_pipe(&pipe_handle_connect_write).expect("无法连接命名管道写端");
    });

    debug_println!("开始连接命名管道和启动 pty-proxy-child");

    // 启动 pty-proxy-child
    let child_process = create_independent_process(
        &format!(
            "\"{}\" {} {} \"{}\" {}",
            child_program,
            pipe_uuid_read,
            pipe_uuid_write,
            target_program,
            target_args
        )
    ).expect("无法启动 pty-proxy-child.exe");

    connect_pipe_thread_handle_read.join().expect("无法 join 读管道连接线程");
    connect_pipe_thread_handle_write.join().expect("无法 join 写管道连接线程");
    debug_println!("连接命名管道完成");

    // 启动线程监听 stdin 并转发给 pty-proxy-child
    let pipe_handle_stdin = Arc::clone(&pipe_handle_write);
    thread::spawn(move || {
        let mut stdin = io::stdin();
        let mut buffer = [0u8; 1024];
        loop {
            let n = stdin.read(&mut buffer).expect("无法读取 stdin");
            if n == 0 {
                break;
            }
            write_to_pipe(&pipe_handle_stdin, &buffer[..n]).expect("无法写入命名管道");
            debug_println!("写入命名管道成功");
        }
    });

    // 启动线程接收来自 pty-proxy-child 的数据并输出到 stdout
    let pipe_handle_stdout = Arc::clone(&pipe_handle_read);
    thread::spawn(move || {
        let mut stdout = io::stdout();
        let mut buffer = [0u8; 1024];
        loop {
            let n = read_from_pipe(&pipe_handle_stdout, &mut buffer).expect("无法读取命名管道");
            if n == 0 {
                break;
            }
            stdout.write_all(&buffer[..n]).expect("无法写入 stdout");
            stdout.flush().expect("无法刷新 stdout");
        }
    });

    // 等待 pty-proxy-child 进程结束
    unsafe {
        WaitForSingleObject(child_process.as_raw_handle() as HANDLE, INFINITE);
    }
    // 获取进程退出代码
    let mut exit_code: u32 = 0;
    let success = unsafe {
        GetExitCodeProcess(child_process.as_raw_handle() as HANDLE, &mut exit_code)
    };
    if success != 0 {
        debug_println!("子进程退出，退出代码：{}，本进程也跟随退出...", exit_code);
        exit(exit_code as i32);
    } else {
        panic!("获取进程退出代码失败：{:?}", io::Error::last_os_error());
    }
}

// 创建命名管道
fn create_named_pipe_read(pipe_name: &str) -> io::Result<OwnedHandle> {
    let pipe_name = to_wstr(pipe_name);
    let pipe_handle: HANDLE = unsafe {
        CreateNamedPipeW(
            pipe_name.as_ptr(),
            PIPE_ACCESS_INBOUND,
            PIPE_READMODE_BYTE | PIPE_WAIT,
            1,
            4096,
            4096,
            0,
            null_mut()
        )
    };
    if pipe_handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    unsafe { Ok(OwnedHandle::from_raw_handle(pipe_handle as *mut c_void)) }
}
fn create_named_pipe_write(pipe_name: &str) -> io::Result<OwnedHandle> {
    let pipe_name = to_wstr(pipe_name);
    let pipe_handle: HANDLE = unsafe {
        CreateNamedPipeW(
            pipe_name.as_ptr(),
            PIPE_ACCESS_OUTBOUND,
            PIPE_TYPE_BYTE | PIPE_WAIT,
            1,
            4096,
            4096,
            0,
            null_mut()
        )
    };
    if pipe_handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    unsafe { Ok(OwnedHandle::from_raw_handle(pipe_handle as *mut c_void)) }
}

// 连接命名管道
fn connect_named_pipe(pipe_handle: &Arc<Mutex<OwnedHandle>>) -> io::Result<()> {
    let pipe_handle = pipe_handle.lock().unwrap();
    let result = unsafe { ConnectNamedPipe(pipe_handle.as_raw_handle() as HANDLE, null_mut()) };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

// 写入命名管道
fn write_to_pipe(pipe_handle: &Arc<Mutex<OwnedHandle>>, data: &[u8]) -> io::Result<()> {
    let pipe_handle = pipe_handle.lock().unwrap();
    let mut bytes_written: u32 = 0;
    let result = unsafe {
        WriteFile(
            pipe_handle.as_raw_handle() as HANDLE,
            data.as_ptr() as *const _,
            data.len() as u32,
            &mut bytes_written,
            null_mut()
        )
    };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

// 从命名管道读取
fn read_from_pipe(pipe_handle: &Arc<Mutex<OwnedHandle>>, buffer: &mut [u8]) -> io::Result<usize> {
    let pipe_handle = pipe_handle.lock().unwrap();
    let mut bytes_read: u32 = 0;
    let result = unsafe {
        ReadFile(
            pipe_handle.as_raw_handle() as HANDLE,
            buffer.as_mut_ptr() as *mut _,
            buffer.len() as u32,
            &mut bytes_read,
            null_mut()
        )
    };

    if result == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
            // 管道已断开连接，返回 0
            return Ok(0);
        } else {
            // 其他错误，返回错误
            return Err(error);
        }
    }

    Ok(bytes_read as usize)
}
