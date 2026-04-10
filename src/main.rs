use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, Write, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;
use winreg::enums::*;
use winreg::RegKey;
use regex::Regex;

/// Modify the Windows Registry to add context menu entries
fn write_key() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path = env::current_exe()?;
    let exe_path_str = exe_path.to_string_lossy().to_string();
    let cmd_value = format!("\"{}\" \"%1\"", exe_path_str);

    // Add command for zip files
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let zip_key = hkcr.create_subkey(r"SystemFileAssociations\.zip\shell\Open Signals\command")?;
    zip_key.0.set_value("", &cmd_value)?;

    // Add command for directories
    let dir_key = hkcr.create_subkey(r"Directory\shell\Open Signal\command")?;
    dir_key.0.set_value("", &cmd_value)?;

    Ok(())
}

/// Waves structure to manage waveform files and GTKWave operations
struct Waves {
    waves_list: Vec<String>,
    pwd: PathBuf,
    signal_list: Vec<String>,
    tcl_file: PathBuf,
    tcl_template: String,
}

impl Waves {
    fn new(path_list: Vec<String>) -> Self {
        let pwd = env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        let tcl_file = env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("add_signal.tcl");

        let mut waves = Waves {
            waves_list: path_list,
            pwd,
            signal_list: Vec::new(),
            tcl_file,
            tcl_template: String::new(),
        };

        waves.read_signal_list();
        waves.build_tcl_template();
        waves
    }

    /// Read signal list from user's home directory
    fn read_signal_list(&mut self) {
        if let Some(home_dir) = home::home_dir() {
            let signal_list_path = home_dir.join("signal.gtkw");
            if signal_list_path.exists() {
                if let Ok(file) = File::open(&signal_list_path) {
                    let reader = BufReader::new(file);
                    let re = Regex::new(r"^[a-zA-Z]").unwrap();

                    for line in reader.lines().flatten() {
                        if re.is_match(&line) {
                            self.signal_list.push(line.trim().to_string());
                        }
                    }
                }
            } else {
                println!("no signal.gtkw file");
            }
        }
    }

    /// Build the TCL template for GTKWave
    fn build_tcl_template(&mut self) {
        let mut template = String::from(
r#"
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
"#);

        if !self.signal_list.is_empty() {
            let ex_facs = self.signal_list
                .iter()
                .map(|item| format!("{{{}}}", item))
                .collect::<Vec<_>>()
                .join(" ");
            template.push_str(&format!("set ex_facs [list {}]\ngtkwave::addSignalsFromList $ex_facs\n", ex_facs));
        }

        template.push_str("gtkwave::addSignalsFromList $all_facs\ngtkwave::/Time/Zoom/Zoom_Full\n}\n");
        self.tcl_template = template;
    }

    /// Write TCL script for GTKWave
    fn wr_tcl(&self) -> Result<(), io::Error> {
        let template_path = self.pwd.join("add_signal_template.tcl");
        let content = if template_path.exists() {
            fs::read_to_string(&template_path)?
        } else {
            self.tcl_template.clone()
        };

        let mut file = File::create(&self.tcl_file)?;
        println!("{}", self.tcl_file.display());
        file.write_all(content.as_bytes())?;

        for sig in &self.waves_list {
            let sig_normalized = sig.replace('\\', "/");
            writeln!(file, "gtkwave::loadFile \"{}\" ", sig_normalized)?;
            writeln!(file, "add_sig")?;
        }

        writeln!(file, "gtkwave::setTabActive 0")?;
        Ok(())
    }

    /// Delete TCL file
    fn del_tcl(&self) -> Result<(), io::Error> {
        let tcl_path = Path::new("./add_signal.tcl");
        if tcl_path.exists() {
            fs::remove_file(tcl_path)?;
        }
        Ok(())
    }

    /// Launch GTKWave
    fn launch_gtkwave(&self) -> Result<(), io::Error> {
        let gtkwave_path = self.pwd.join("gtkwave.exe");
        let cmd_str = format!("{} -T ./add_signal.tcl", gtkwave_path.display());

        Command::new("cmd")
            .args(&["/C", &cmd_str])
            .output()?;

        Ok(())
    }

    /// Execute the full workflow: write TCL, launch GTKWave, delete TCL
    fn execute(&self) -> Result<(), io::Error> {
        self.wr_tcl()?;
        self.launch_gtkwave()?;
        self.del_tcl()?;
        Ok(())
    }
}

/// Recursively find VCD files in a directory
fn trace_vcd(path: &Path, results: &mut HashMap<PathBuf, Vec<String>>) {
    for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() {
            if let Some(file_name) = path.file_name() {
                let file_name_str = file_name.to_string_lossy();
                if file_name_str.ends_with(".vcd") && !file_name_str.starts_with("diags") {
                    if let Some(parent) = path.parent() {
                        let parent_buf = parent.to_path_buf();
                        let _relative_path = path.strip_prefix(&parent_buf)
                            .unwrap_or(path)
                            .to_string_lossy()
                            .to_string();

                        results.entry(parent_buf)
                            .or_insert_with(Vec::new)
                            .push(path.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
}

/// Find all VCD files and organize them by directory
fn find_vcd(path: &Path) -> HashMap<PathBuf, Vec<String>> {
    let mut results = HashMap::new();
    trace_vcd(path, &mut results);
    results
}

/// Unpack a zip file or return the directory path
fn unpack_file(tar_file: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = Path::new(tar_file);

    if tar_file.ends_with(".zip") {
        let home_dir = home::home_dir().ok_or("Cannot find home directory")?;
        let unpack_path = home_dir.join("OpenSignal");

        // Create the OpenSignal directory if it doesn't exist
        fs::create_dir_all(&unpack_path)?;

        let mut unp = unpack_path.join("temp0");

        // Try to use temp0, or find the next available temp directory
        if unp.exists() {
            if let Err(_) = fs::remove_dir_all(&unpack_path) {
                // If we can't remove the directory, find the next available temp number
                let entries = fs::read_dir(&unpack_path)?;
                let mut max_num = 0;

                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    let name = file_name.to_string_lossy();
                    if name.starts_with("temp") {
                        if let Ok(num) = name[4..].parse::<i32>() {
                            max_num = max_num.max(num);
                        }
                    }
                }

                unp = unpack_path.join(format!("temp{}", max_num + 1));
            } else {
                unp = unpack_path.join("temp0");
            }
        }

        // Extract the zip file
        let file = File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        archive.extract(&unp)?;

        Ok(unp)
    } else {
        Ok(path.to_path_buf())
    }
}

/// Main run function
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    // If no arguments, initialize registry keys
    if args.len() == 1 {
        match write_key() {
            Ok(_) => {
                println!("Init Over!!");
                println!("Press Enter to continue...");
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                println!("Press Enter to continue...");
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                return Err(e);
            }
        }
    } else {
        // Process each argument (file or directory)
        for arg in &args[1..] {
            let unpacked_path = unpack_file(arg)?;
            let vcd_files = find_vcd(&unpacked_path);

            for (dir, files) in vcd_files.iter() {
                if !dir.as_os_str().is_empty() {
                    env::set_current_dir(dir)?;
                }

                let waves = Waves::new(files.clone());
                waves.execute()?;
            }
        }
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
