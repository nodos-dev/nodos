use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Output;
use zip::ZipArchive;
use crate::nosman::command::CommandError;

pub fn download_and_extract(url: &str, target: &PathBuf) -> Result<(), CommandError> {
    let mut tmpfile = tempfile::tempfile().expect("Failed to create tempfile");
    reqwest::blocking::get(url)
    .expect(format!("Failed to fetch {}", url).as_str()).copy_to(&mut tmpfile)
    .expect(format!("Failed to write to {:?}", tmpfile).as_str());

    let mut archive = ZipArchive::new(tmpfile)?;
    fs::create_dir_all(target.clone())?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = Path::new(&target.clone()).join(file.name());

        if file.is_dir() {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            let mut outfile = fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }
    Ok(())
}

pub fn check_file_contents_same(path1: &PathBuf, path2: &PathBuf) -> bool {
    // Efficiently compare file contents
    let mut file1 = File::open(path1).expect(format!("Failed to open {:?}", path1).as_str());
    let mut file2 = File::open(path2).expect(format!("Failed to open {:?}", path2).as_str());
    let mut buf1 = [0; 1024];
    let mut buf2 = [0; 1024];
    if file1.metadata().unwrap().len() != file2.metadata().unwrap().len() {
        return false;
    }
    loop {
        let n1 = file1.read(&mut buf1).expect(format!("Failed to read {}", path1.display()).as_str());
        let n2 = file2.read(&mut buf2).expect(format!("Failed to read {}", path2.display()).as_str());
        if n1 != n2 || buf1 != buf2 {
            return false;
        }
        if n1 == 0 {
            break;
        }
    }
    true
}

pub fn ask(question: &str, default: bool, do_default: bool) -> bool {
    let mut answer = String::new();
    loop {
        let default_str = if default { "Y/n" } else { "y/N" };
        print!("{} [{}]: ", question, default_str);
        std::io::stdout().flush().unwrap();
        std::io::stdin().read_line(&mut answer).unwrap();
        answer = answer.trim().to_lowercase();
        if answer == "y" {
            return true;
        } else if answer == "n" {
            return false;
        } else if answer.is_empty() {
            return do_default;
        } else {
            println!("Invalid input, please enter 'y' or 'n'");
        }
    }
}

pub fn run_if_not(dry_run: bool, verbose: bool, cmd: &mut std::process::Command) -> Option<Output> {
    if dry_run {
        println!("Would run: {:?}", cmd);
        None
    } else {
        if verbose {
            println!("Running: {:?}", cmd);
        }
        let res = cmd.output();
        if verbose {
            if res.is_ok() {
                let output = res.as_ref().unwrap();
                println!("{}:\n{}", if output.status.success() { "stdout" } else { "stderr" },
                         String::from_utf8_lossy(if output.status.success() { &output.stdout } else { &output.stderr }));
            }
        }
        Some(res.expect(format!("Failed to run command {:?}", cmd).as_str()))
    }
}