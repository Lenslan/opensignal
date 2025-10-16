use std::{
    env,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};
use winreg::{
    enums::HKEY_CLASSES_ROOT,
    RegKey,
};
use zip::ZipArchive;
use regex::Regex;
use walkdir::WalkDir;
use home::home_dir;

// 修改注册表
fn write_key() -> io::Result<()> {
    let current_exe_path = env::current_exe()?;
    let script_path = current_exe_path.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "无法获取可执行文件路径")
    })?;

    // 为 .zip 文件添加右键菜单
    let zip_key_path = r"SystemFileAssociations\.zip\shell\Open Signals\command";
    let (zip_key, _) = RegKey::predef(HKEY_CLASSES_ROOT) // 解构元组
        .create_subkey(zip_key_path)?;
    let cmd_value_zip = format!("\"{}\" \"%1\"", script_path);
    zip_key.set_value("", &cmd_value_zip)?;

    // 为目录添加右键菜单
    let dir_key_path = r"Directory\shell\Open Signal\command";
    let (dir_key, _) = RegKey::predef(HKEY_CLASSES_ROOT) // 解构元组
        .create_subkey(dir_key_path)?;
    let cmd_value_dir = format!("\"{}\" \"%1\"", script_path);
    dir_key.set_value("", &cmd_value_dir)?;

    // 为文件夹背景添加右键菜单
    let dir_background_key_path = r"Directory\Background\shell\Open Signal\command";
    let (dir_background_key, _) = RegKey::predef(HKEY_CLASSES_ROOT)
        .create_subkey(dir_background_key_path)?;
    let cmd_value_dir_background = format!("\"{}\" \"%V\"", script_path);
    dir_background_key.set_value("", &cmd_value_dir_background)?;

    Ok(())
}

// 解压文件或返回原始路径
fn unpack_file(tar_file: &Path) -> io::Result<PathBuf> {
    if tar_file.extension().map_or(false, |ext| ext == "zip") {
        let home_dir = home_dir().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "无法获取用户主目录")
        })?;
        let unpack_base_path = home_dir.join("OpenSignal");

        // 确保 OpenSignal 目录存在
        fs::create_dir_all(&unpack_base_path)?;

        let mut unpack_path = PathBuf::new();
        let mut temp_num = 0;
        loop {
            unpack_path = unpack_base_path.join(format!("temp{}", temp_num));
            if !unpack_path.exists() {
                break;
            }
            temp_num += 1;
        }

        let file = fs::File::open(tar_file)?;
        let mut archive = ZipArchive::new(file)?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let outpath = match file.enclosed_name() {
                Some(path) => unpack_path.join(path),
                None => continue,
            };

            if (*file.name()).ends_with('/') {
                fs::create_dir_all(&outpath)?;
            } else {
                if let Some(p) = outpath.parent() {
                    fs::create_dir_all(p)?;
                }
                let mut outfile = fs::File::create(&outpath)?;
                io::copy(&mut file, &mut outfile)?;
            }
        }
        Ok(unpack_path)
    } else {
        Ok(tar_file.to_path_buf())
    }
}

// 查找 *.vcd 文件
fn find_vcd(path: &Path) -> io::Result<std::collections::HashMap<PathBuf, Vec<PathBuf>>> {
    let mut result: std::collections::HashMap<PathBuf, Vec<PathBuf>> = std::collections::HashMap::new();

    for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        let entry_path = entry.path();
        if entry_path.is_file() {
            if let Some(file_name) = entry_path.file_name().and_then(|s| s.to_str()) {
                if file_name.ends_with(".vcd") && !file_name.starts_with("diags") {
                    if let Some(parent) = entry_path.parent() {
                        result.entry(parent.to_path_buf())
                              .or_insert_with(Vec::new)
                              .push(entry_path.to_path_buf());
                    }
                }
            }
        }
    }
    Ok(result)
}

// Waves 操作
struct Waves {
    waves_list: Vec<PathBuf>,
    pwd: PathBuf,
    signal_list: Vec<String>,
    tcl_file: PathBuf,
    tcl_template: String,
}

impl Waves {
    fn new(path_list: Vec<PathBuf>) -> io::Result<Self> {
        let pwd = env::current_exe()?
            .parent()
            .map_or_else(|| PathBuf::from("."), |p| p.to_path_buf());
        let tcl_file = env::current_dir()?.join("add_signal.tcl");
        let mut waves = Waves {
            waves_list: path_list,
            pwd,
            signal_list: Vec::new(),
            tcl_file,
            tcl_template: String::new(), // 初始为空，后续填充
        };
        waves.read_signal_list()?;
        waves.init_tcl_template();
        Ok(waves)
    }

    // 读取信号列表
    fn read_signal_list(&mut self) -> io::Result<()> {
        let signal_list_path = home_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "无法获取用户主目录"))?
            .join("signal.gtkw");

        if !signal_list_path.exists() {
            println!("no signal.gtkw file");
            return Ok(());
        }

        let content = fs::read_to_string(&signal_list_path)?;
        let re = Regex::new(r"^[a-zA-Z]").unwrap();
        for line in content.lines() {
            if re.is_match(line) {
                self.signal_list.push(line.trim().to_string());
            }
        }
        Ok(())
    }

    // 初始化 TCL 模板
    fn init_tcl_template(&mut self) {
        let mut template = r#"
proc add_sig {} {
    set nfacs [ gtkwave::getNumFacs ]
    set all_facs [list]
    for {set i 0} {$i < $nfacs } {incr i} {
        set facname [gtkwave::getFacName $i]
        set facname2 [gtkwave::getFacName $i]
        set changes [ gtkwave::signalChangeList $facname2 -max 1 ]
        set no_x 1
        foreach {time value} $changes {
            set firststr [string range $value 0 2]
            if {$value eq "x" || $firststr eq "0xx" || $firststr eq "0bx" } {
                set no_x 0
                break
            }
        }
        if {$no_x} {lappend all_facs "$facname"}
    }
"#.to_string();

        let custom_template_path = self.pwd.join("add_signal_template.tcl");
        if custom_template_path.exists() {
            if let Ok(custom_template) = fs::read_to_string(&custom_template_path) {
                template = custom_template;
            }
        }

        if !self.signal_list.is_empty() {
            let signals_str = self.signal_list
                .iter()
                .map(|s| format!("{{{}}}", s))
                .collect::<Vec<String>>()
                .join(" ");
            template = format!("{}set ex_facs [list {}]\ngtkwave::addSignalsFromList $ex_facs\n", template, signals_str);
        }
        template.push_str("gtkwave::addSignalsFromList $all_facs\ngtkwave::/Time/Zoom/Zoom_Full\n}\n");
        self.tcl_template = template;
    }

    // 写入 TCL 脚本
    fn write_tcl(&self) -> io::Result<&Self> {
        println!("{}", self.tcl_file.display());
        let mut file = fs::File::create(&self.tcl_file)?;
        file.write_all(self.tcl_template.as_bytes())?;

        for sig in &self.waves_list {
            let sig_str = sig.to_str().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "无效的 VCD 路径")
            })?.replace('\\', "/");
            file.write_all(format!("gtkwave::loadFile \"{}\" \n", sig_str).as_bytes())?;
            file.write_all(b"add_sig\n")?;
        }
        file.write_all(b"gtkwave::setTabActive 0")?;
        Ok(self)
    }

    // 删除 TCL 文件
    fn delete_tcl(&self) -> io::Result<&Self> {
        if self.tcl_file.exists() {
            fs::remove_file(&self.tcl_file)?;
        }
        Ok(self)
    }

    // 启动 gtkwave
    fn launch_gtkwave(&self) -> io::Result<&Self> {
        let gtkwave_cmd = self.pwd.join("gtkwave.exe");
        let tcl_file_arg = format!("-T {}", self.tcl_file.display());

        Command::new(&gtkwave_cmd)
            .arg(&tcl_file_arg)
            .spawn()?
            .wait()?;
        Ok(self)
    }
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() == 1 {
        write_key()?;
    } else {
        // 处理命令行参数
        for arg in &args[1..] {
            let input_path = PathBuf::from(arg);
            let unpacked_path = unpack_file(&input_path)?;
            let vcd_files_by_dir = find_vcd(&unpacked_path)?;

            for (dir_path, vcd_list) in vcd_files_by_dir {
                // 切换当前工作目录
                env::set_current_dir(&dir_path)?;

                let waves = Waves::new(vcd_list)?;
                waves.write_tcl()?
                     .launch_gtkwave()?
                     .delete_tcl()?;

            }
        }
    }

    Ok(())
}
