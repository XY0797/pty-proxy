use std::ffi::{ OsString, c_void };
use std::process::exit;
use std::sync::{ Arc, Mutex, mpsc };
use std::io::{ self };
use std::os::windows::io::{ AsRawHandle, OwnedHandle, FromRawHandle };
use std::ptr::null_mut;
use std::io::{ Read, Write };
use regex::Regex;
use chrono::Local;

use winptyrs::{ PTY, PTYArgs, MouseMode, AgentConfig, PTYBackend };
use windows_sys::{ Win32::Foundation::*, Win32::Storage::FileSystem::* };

macro_rules! debug_println {
    ($($arg:tt)*) => {
        #[cfg(feature = "debug_mode")]
        {
            println!($($arg)*);
        }
    };
}
macro_rules! debug_pause {
    ($($arg:tt)*) => {
        #[cfg(feature = "debug_mode")]
        {
            println!($($arg)*);
            let mut buf = vec![0;1];
            use std::io::Read;
            io::stdin().lock().read_exact(&mut buf).unwrap();
        }
    };
}

fn main() {
    #[cfg(feature = "debug_mode")]
    {
        use std::panic;
        panic::set_hook(
            Box::new(|panic_info| {
                println!("\n{}", panic_info);
                debug_pause!("Press enter to exit...");
            })
        );
    }
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!(
            "用法: pty-proxy-child <pipe_uuid_write> <pipe_uuid_read> <target_program> [args...]"
        );
        panic!("至少要3个命令行参数才能运行！");
    }

    let pipe_uuid_write = &args[1];
    let pipe_uuid_read = &args[2];
    let pipe_name_write = format!(r"\\.\pipe\ptyproxy{}", pipe_uuid_write);
    let pipe_name_read = format!(r"\\.\pipe\ptyproxy{}", pipe_uuid_read);
    let target_program = &args[3];
    let target_args = args[4..].join(" ");

    println!("虚拟终端代理-子程序  请不要关闭本窗口！");
    println!("pty-proxy-child  please DO NOT close this window!");
    println!();
    println!("pipe_uuid_write: {}", pipe_uuid_write);
    println!("pipe_uuid_read: {}", pipe_uuid_read);
    println!("target_program: {}", target_program);
    println!("target_args: {}", target_args);
    println!();

    // 连接到命名管道
    let pipe_handle_write = Arc::new(
        Mutex::new(
            connect_to_named_pipe_write(pipe_name_write.as_str()).expect("无法连接到命名管道写端")
        )
    ); // 包装为线程安全
    let pipe_handle_read = Arc::new(
        Mutex::new(
            connect_to_named_pipe_read(pipe_name_read.as_str()).expect("无法连接到命名管道读端")
        )
    ); // 包装为线程安全

    println!("工作中...");
    println!("working...");

    // 创建 PTY
    let pty_args = PTYArgs {
        cols: 1024,
        rows: 2,
        mouse_mode: MouseMode::WINPTY_MOUSE_MODE_NONE,
        timeout: 10000,
        agent_config: AgentConfig::WINPTY_FLAG_COLOR_ESCAPES,
    };

    #[cfg(feature = "winpty")]
    let pty_backend = PTYBackend::WinPTY;

    #[cfg(not(feature = "winpty"))]
    let pty_backend = PTYBackend::ConPTY;

    let pty = Arc::new(
        Mutex::new(PTY::new_with_backend(&pty_args, pty_backend).expect("无法创建 PTY"))
    );

    // 启动目标进程
    pty.lock()
        .unwrap()
        .spawn(
            OsString::from(target_program),
            if target_args.is_empty() {
                None
            } else {
                Some(OsString::from(target_args))
            },
            None,
            None
        )
        .expect("无法启动目标进程");

    debug_println!("目标进程启动成功");

    // 启动线程读取 PTY 输出并发送到命名管道
    let pty_output = pty.clone();
    let pipe_handle_output = Arc::clone(&pipe_handle_write);
    let ptyread_thread_handle = std::thread::spawn(move || {
        // 创建日志文件
        let mut log_file = std::fs::OpenOptions
            ::new()
            .create(true)
            .append(true)
            .open("output_log_raw.txt")
            .expect("无法创建或打开日志文件");
        loop {
            // 读取一轮数据并发送到命名管道
            {
                let output: OsString = {
                    let pty = pty_output.lock().unwrap();
                    pty.read(1000, false).expect("无法读取 PTY 输出")
                };
                if !output.is_empty() {
                    debug_println!("收到数据，转发..");
                    let output_str = output.to_string_lossy();
                    write_to_pipe(&pipe_handle_output, output_str.as_bytes()).expect(
                        "无法写入命名管道"
                    );
                    // 写入日志文件
                    writeln!(log_file, "{}", output_str).expect("无法写入日志文件");
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(100));

            // 检查目标进程是否已退出
            let is_alive = {
                let pty = pty_output.lock().unwrap();
                pty.is_alive().unwrap_or(false)
            };

            if !is_alive {
                debug_println!("监听到进程退出");
                // 读取pty中剩下的序列
                loop {
                    let output = {
                        let pty = pty_output.lock().unwrap();
                        pty.read(1000, false)
                    };
                    match output {
                        Ok(output) => {
                            if output.is_empty() {
                                return;
                            } else {
                                let output_str = output.to_string_lossy();
                                write_to_pipe(&pipe_handle_output, output_str.as_bytes()).expect(
                                    "无法写入命名管道"
                                );
                            }
                        }
                        Err(_) => {
                            return;
                        }
                    }
                }
            }
        }
    });

    // 启动线程从命名管道读取输入并写入 PTY
    let pty_input = pty.clone();
    let pipe_handle_input = Arc::clone(&pipe_handle_read);
    let ptywrite_thread_handle = std::thread::spawn(move || {
        let mut buffer = [0u8; 1024];
        loop {
            match read_from_pipe(&pipe_handle_input, &mut buffer) {
                Ok(n) => {
                    if n == 0 {
                        break;
                    }
                    let input = String::from_utf8_lossy(&buffer[..n]).to_string();
                    debug_println!("收到输入数据");
                    {
                        pty_input
                            .lock()
                            .unwrap()
                            .write(OsString::from(input))
                            .expect("无法写入 PTY");
                    }
                    debug_println!("成功写入PTY");
                }
                Err(e) => {
                    panic!("无法读取命名管道: {e:?}");
                }
            }
        }
    });

    let (tx1, rx) = mpsc::channel();
    let tx2 = tx1.clone();
    std::thread::spawn(move || {
        let _ = ptyread_thread_handle.join();
        if let Err(e) = tx1.send(()) {
            eprintln!("无法发送进程结束信号: {}", e);
            exit(101);
        }
    });
    std::thread::spawn(move || {
        let _ = ptywrite_thread_handle.join();
        if let Err(e) = tx2.send(()) {
            eprintln!("无法发送进程结束信号: {}", e);
            exit(101);
        }
    });

    // 等待 PTY 进程结束
    rx.recv().expect("无法接收进程结束信号");
    let exit_status = pty.lock().unwrap().get_exitstatus().expect("无法获取 PTY 退出状态");
    // 在退出前处理日志文件
    if let Ok(mut file) = std::fs::File::open("output_log_raw.txt") {
        let mut content = String::new();
        if file.read_to_string(&mut content).is_ok() {
            let re = Regex::new(r"\x1B\[(.*?)[A-Za-z]").unwrap();
            let cleaned = re.replace_all(&content, "");

            // 获取当前时间并格式化为 YYYYMMDDHHmm 格式
            let now = Local::now();
            let timestamp = now.format("%Y%m%d%H%M%S").to_string();
            let new_filename = format!("output_log_{}.txt", timestamp);

            if let Ok(mut file) = std::fs::File::create(&new_filename) {
                let _ = file.write_all(cleaned.as_bytes());
            }
            // 删除原始日志文件
            let _ = std::fs::remove_file("output_log_raw.txt");
        }
    }
    debug_pause!("进程即将退出，退出代码：{}，按回车键退出...", exit_status.unwrap_or(101));
    exit(exit_status.unwrap_or(101) as i32);
}

// 连接到命名管道
fn connect_to_named_pipe_write(pipe_name: &str) -> io::Result<OwnedHandle> {
    let pipe_name = pipe_name.as_bytes();
    let pipe_handle = unsafe {
        CreateFileA(
            pipe_name.as_ptr(),
            GENERIC_WRITE,
            0,
            null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            null_mut()
        )
    };
    if pipe_handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    unsafe { Ok(OwnedHandle::from_raw_handle(pipe_handle as *mut c_void)) }
}
fn connect_to_named_pipe_read(pipe_name: &str) -> io::Result<OwnedHandle> {
    let pipe_name = pipe_name.as_bytes();
    let pipe_handle = unsafe {
        CreateFileA(pipe_name.as_ptr(), GENERIC_READ, 0, null_mut(), OPEN_EXISTING, 0, null_mut())
    };
    if pipe_handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    unsafe { Ok(OwnedHandle::from_raw_handle(pipe_handle as *mut c_void)) }
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
        return Err(io::Error::last_os_error());
    }
    Ok(bytes_read as usize)
}
