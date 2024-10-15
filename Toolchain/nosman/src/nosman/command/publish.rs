use std::fs::File;
use std::io::{Read};
#[cfg(unix)]
use std::env;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(target_os = "windows")]
use std::io::{Write};
use std::path;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;
use clap::{ArgMatches};
use colored::Colorize;
use indicatif::ProgressBar;
use libloading::{Library, Symbol};
use serde::{Deserialize, Serialize};
use tempfile::{tempdir};
#[cfg(target_os = "windows")]
use zip::write::{SimpleFileOptions};
use chrono::{Utc};

use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::command::CommandError::{GenericError, InvalidArgumentError};
use crate::nosman::constants;
use crate::nosman::index::{PackageReleaseEntry, PackageType, SemVer};
use crate::nosman::module::PackageIdentifier;
use crate::nosman::path::{get_plugin_manifest_file, get_subsystem_manifest_file};
use crate::nosman::platform::{get_host_platform, Platform};
use crate::nosman::workspace::Workspace;

#[derive(Serialize, Deserialize, Debug)]
pub struct PublishOptions {
    #[serde(alias = "globs")]
    pub(crate) release_globs: Vec<String>,
    #[serde(alias = "trigger_publish_globs")]
    pub(crate) additional_publish_triggering_globs: Option<Vec<String>>,
    pub(crate) target_platforms: Option<Vec<String>>,
}

impl PublishOptions {
    pub fn from_file(nospub_file: &PathBuf) -> (PublishOptions, bool) {
        let mut nospub = PublishOptions { release_globs: vec![], additional_publish_triggering_globs: None, target_platforms: None };
        let found = nospub_file.exists();
        if found {
            let contents = std::fs::read_to_string(&nospub_file).unwrap();
            nospub = serde_json::from_str(&contents).unwrap();
        }
        else {
            nospub.release_globs.push("**".to_string());
        }
        (nospub, found)
    }
    pub fn empty() -> PublishOptions {
        PublishOptions { release_globs: vec![], additional_publish_triggering_globs: None, target_platforms: None }
    }
}

pub struct PublishCommand {
}

impl PublishCommand {
    fn load_module_with_search_paths(verbose: bool, binary_path: &OsString, additional_search_paths: Vec<PathBuf>) -> Result<Library, CommandError> {
        if verbose {
            println!("Loading dynamic library: {}", binary_path.to_str().unwrap());
        }
        #[cfg(unix)]
        {
            // Store the original environment variable values
            #[cfg(target_os = "linux")]
            let original_var = env::var_os("LD_LIBRARY_PATH");

            #[cfg(target_os = "macos")]
            let original_var = env::var_os("DYLD_LIBRARY_PATH");


            {
                for lib_dir in additional_search_paths {
                    // Add this directory to the appropriate environment variable
                    #[cfg(target_os = "linux")]
                    {
                        let paths = env::var_os("LD_LIBRARY_PATH").unwrap_or_else(|| "".into());
                        let mut lib_dir = lib_dir.clone();
                        lib_dir.push(":");
                        lib_dir.push(paths);
                        env::set_var("LD_LIBRARY_PATH", lib_dir);
                    }

                    #[cfg(target_os = "macos")]
                    {
                        let paths = env::var_os("DYLD_LIBRARY_PATH").unwrap_or_else(|| "".into());
                        let mut lib_dir = lib_dir.clone();
                        lib_dir.push(":");
                        lib_dir.push(paths);
                        env::set_var("DYLD_LIBRARY_PATH", lib_dir);
                    }
                }
            }



            let res;
            // Now load the library
            unsafe {
                res = Library::new(&binary_path)
            }

            {
                // Restore the original environment variable values
                #[cfg(target_os = "linux")]
                if let Some(original) = original_var {
                    env::set_var("LD_LIBRARY_PATH", original);
                } else {
                    env::remove_var("LD_LIBRARY_PATH");
                }

                #[cfg(target_os = "macos")]
                if let Some(original) = original_var {
                    env::set_var("DYLD_LIBRARY_PATH", original);
                } else {
                    env::remove_var("DYLD_LIBRARY_PATH");
                }
            }

            if res.is_err() {
                return Err(GenericError { message: format!("Failed to load dynamic library: {}", res.err().unwrap()) });
            }
            Ok(res.unwrap())
        }

        #[cfg(target_os = "windows")]
        unsafe {
            // Set default DLL directories
            use winapi::um::libloaderapi::{SetDefaultDllDirectories, AddDllDirectory, RemoveDllDirectory};
            use winapi::um::libloaderapi::LOAD_LIBRARY_SEARCH_DEFAULT_DIRS;
            if 0 == SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS) {
                // Get last error
                let err = std::io::Error::last_os_error();
                return Err(GenericError { message: format!("Failed to set default DLL directories: {}", err) });
            }
            let mut dll_cookies = vec![];
            for lib_dir in additional_search_paths {
                if !lib_dir.exists() {
                    println!("{}", format!("Warning: DLL search path {} does not exist", lib_dir.display()).yellow().to_string());
                    continue;
                }
                let lib_dir_canonical = dunce::canonicalize(&lib_dir).expect(format!("Failed to canonicalize path: {}", lib_dir.display()).as_str());
                if verbose {
                    println!("\tAdding DLL search path: {}", lib_dir_canonical.display());
                }
                let wdir: Vec<u16> = lib_dir_canonical.as_os_str().encode_wide().chain(Some(0)).collect();
                let cookie = AddDllDirectory(wdir.as_ptr());
                if cookie.is_null() {
                    let err = std::io::Error::last_os_error();
                    return Err(GenericError { message: format!("Failed to add DLL search path {}: {}", lib_dir_canonical.display(), err) });
                }
                dll_cookies.push(cookie);
            }
            let res = Library::new(&binary_path);
            for cookie in dll_cookies {
                RemoveDllDirectory(cookie);
            }
            if res.is_err() {
                return Err(GenericError { message: format!("Failed to load dynamic library: {}", res.err().unwrap()) });
            }
            Ok(res.unwrap())
        }
    }
    fn is_name_valid(name: &String) -> bool {
        // Should be lowercase alphanumeric, with only . and _ symbols are permitted
        name.chars().all(|c| c == '.' || c == '_' || c.is_numeric() || c.is_ascii_lowercase())
    }
    pub fn run_publish(&self, dry_run: bool, verbose: bool, path: &PathBuf, mut name: Option<String>, mut version: Option<String>, version_suffix: &String,
                   mut package_type: Option<PackageType>, remote_name: &String, vendor: Option<&String>,
                   publisher_name: Option<&String>, publisher_email: Option<&String>, release_tags: &Vec<String>, opt_target_platform: Option<&String>) -> CommandResult {
        // Check if git and gh is installed.
        let git_installed = std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_ok();
        if !git_installed {
            return Err(GenericError { message: "git is not on PATH".to_string() });
        }
        let gh_installed = std::process::Command::new("gh")
            .arg("--version")
            .output()
            .is_ok();
        if !gh_installed {
            return Err(GenericError { message: "GitHub CLI client 'gh' is not on PATH".to_string() });
        }


        let target_platform = if opt_target_platform.is_none() {
            let current_platform = get_host_platform();
            println!("{}", format!("Target platform is not provided. Using the current platform: {}", current_platform).yellow());
            current_platform
        } else {
            Platform::from_str(opt_target_platform.unwrap()).expect("Invalid target platform")
        };

        if !path.exists() {
            return Err(InvalidArgumentError { message: format!("Path {} does not exist", path.display()) });
        }

        let abs_path = dunce::canonicalize(path).expect(format!("Failed to canonicalize path: {}", path.display()).as_str());

        let mut nospub = PublishOptions::empty();

        let mut api_version: Option<SemVer> = None;
        let mut min_required_minor_opt: Option<u32> = None;

        let mut dependencies: Option<Vec<PackageIdentifier>> = None;
        let mut category: Option<String> = None;
        let mut module_tags: Option<Vec<String>> = None;

        // If path is a directory, search for a manifest file
        let mut manifest_file = None;
        if abs_path.is_dir() {
            let (options, found) = PublishOptions::from_file(&abs_path.join(constants::PUBLISH_OPTIONS_FILE_NAME));
            nospub = options;
            if !found {
                println!("{}", format!("No {} file found in {}. All files will be included in the release.", constants::PUBLISH_OPTIONS_FILE_NAME, abs_path.display()).as_str().yellow());
            } else if let Some(targets) = nospub.target_platforms {
                if !targets.contains(&target_platform.to_string()) {
                    println!("{}", format!("Target platform {} is not in the list of target platforms in {}", target_platform.to_string(), constants::PUBLISH_OPTIONS_FILE_NAME).as_str().yellow());
                    return Ok(false);
                }
            }

            let res = get_plugin_manifest_file(&abs_path);
            if res.is_err() {
                return Err(InvalidArgumentError { message: res.err().unwrap() });
            }
            let plugin_manifest_file = res.unwrap();
            let res = get_subsystem_manifest_file(&abs_path);
            if res.is_err() {
                return Err(InvalidArgumentError { message: res.err().unwrap() });
            }
            let subsystem_manifest_file = res.unwrap();
            if plugin_manifest_file.is_some() && subsystem_manifest_file.is_some() {
                return Err(InvalidArgumentError { message: format!("Multiple module manifest files found in {}", abs_path.display()) });
            }

            if plugin_manifest_file.is_some() {
                package_type = Some(PackageType::Plugin);
            } else if subsystem_manifest_file.is_some() {
                package_type = Some(PackageType::Subsystem);
            }

            manifest_file = plugin_manifest_file.or(subsystem_manifest_file);
            if manifest_file.is_some() {
                let package_type = package_type.as_ref().unwrap();
                let manifest_file = manifest_file.as_ref().unwrap();
                let contents = std::fs::read_to_string(manifest_file).unwrap();
                let manifest: serde_json::Value = serde_json::from_str(&contents).unwrap();
                name = Some(manifest["info"]["id"]["name"].as_str().expect(format!("Module manifest file {:?} must contain info.id.name field!", manifest_file).as_str()).to_string());
                version = Some(manifest["info"]["id"]["version"].as_str().expect(format!("Module manifest file {:?} must contain info.id.version field!", manifest_file).as_str()).to_string());
                let dependencies_json = manifest["info"]["dependencies"].as_array();
                if dependencies_json.is_some() {
                    let mut deps = vec![];
                    for dep in dependencies_json.unwrap() {
                        let dep_name = dep["name"].as_str().unwrap();
                        let dep_version = dep["version"].as_str().unwrap();
                        deps.push(PackageIdentifier { name: dep_name.to_string(), version: dep_version.to_string() });
                    }
                    dependencies = Some(deps);
                }
                category = manifest["info"]["category"].as_str().map(|s| s.to_string());
                module_tags = manifest["info"]["tags"].as_array().map(|a| a.iter().map(|v| v.as_str().unwrap().to_string()).collect());
                let binary_path = manifest["binary_path"].as_str();
                if binary_path.is_some() {
                    // Binary path is relative to the manifest file
                    let module_dir = manifest_file.parent().unwrap();
                    let binary_path = module_dir.join(binary_path.unwrap());
                    let binary_path = binary_path.with_extension(
                        if target_platform.os == "windows" { "dll" }
                        else if target_platform.os == "macos" { "dylib" }
                        else { "so" }
                    ).into_os_string();
                    let mut additional_search_paths: Vec<PathBuf> = Vec::new();
                    for path_str in manifest["additional_search_paths"].as_array().unwrap_or(&vec![]).iter() {
                        let path = module_dir.join(path_str.as_str().unwrap());
                        additional_search_paths.push(path);
                    }
                    // Add search paths of dependencies
                    for dep in manifest["info"]["dependencies"].as_array().unwrap_or(&vec![]) {
                        let dep_name = dep["name"].as_str().unwrap();
                        let dep_version = dep["version"].as_str().unwrap();
                        let ws = Workspace::get()?;
                        let dep_res = ws.get_latest_installed_module_for_version(dep_name, dep_version);
                        if let Ok(installed_module) = dep_res {
                            let dep_manifest_file_path = ws.root.join(&installed_module.manifest_path);
                            let dep_manifest_file_contents = std::fs::read_to_string(&dep_manifest_file_path).expect("Failed to read dependency manifest file");
                            let dep_manifest: serde_json::Value = serde_json::from_str(&dep_manifest_file_contents).expect("Failed to parse dependency manifest file");
                            for path_str in dep_manifest["additional_search_paths"].as_array().unwrap_or(&vec![]) {
                                let module_dir = dep_manifest_file_path.parent().unwrap();
                                let path = module_dir.join(path_str.as_str().unwrap());
                                additional_search_paths.push(path);
                            }
                        }
                    }
                    // Load the dynamic library
                    unsafe {
                        let lib = Self::load_module_with_search_paths(verbose, &binary_path, additional_search_paths);
                        if lib.is_err() {
                            return Err(InvalidArgumentError { message: format!("Could not load dynamic library {}: {}. \
                                Make sure all the dependencies are present in the system and the search paths.", &binary_path.to_str().unwrap(), lib.err().unwrap()) });
                        }
                        if verbose {
                            println!("Module {} loaded successfully. Checking Nodos {:?} API version...", name.as_ref().unwrap(), &package_type);
                        }
                        let lib = lib.unwrap();
                        {
                            let get_api_version_func_name = match package_type {
                                PackageType::Plugin => "nosGetPluginAPIVersion",
                                PackageType::Subsystem => "nosGetSubsystemAPIVersion",
                                _ => panic!("Invalid package type")
                            };
                            let get_api_version_func = lib.get::<Symbol<unsafe extern "C" fn(*mut i32, *mut i32, *mut i32)>>(get_api_version_func_name.as_bytes()).expect(format!("Failed to get symbol {}", get_api_version_func_name).as_str());
                            let mut major = 0;
                            let mut minor = 0;
                            let mut patch = 0;
                            get_api_version_func(&mut major, &mut minor, &mut patch);
                            api_version = Some(SemVer { major: (major as u32), minor: Some(minor as u32), patch: Some(patch as u32), build_number: None });
                            println!("{}", format!("{} uses Nodos {:?} API version: {}.{}.{}", name.as_ref().unwrap(), &package_type, major, minor, patch).as_str().yellow());
                        }
                        {
                            let get_min_required_minor_func_name = match package_type {
                                PackageType::Plugin => "nosGetMinimumRequiredPluginAPIMinorVersion",
                                PackageType::Subsystem => "nosGetMinimumRequiredPluginAPIMinorVersion",
                                _ => panic!("Invalid package type")
                            };
                            if let Ok(get_min_required_minor_func) = lib.get::<Symbol<unsafe extern "C" fn(*mut i32)>>(get_min_required_minor_func_name.as_bytes()) {
                                let mut min_required_minor: i32 = 0;
                                get_min_required_minor_func(&mut min_required_minor);
                                if min_required_minor > 0 {
                                    min_required_minor_opt = Some(min_required_minor as u32);
                                    println!("{}", format!("{} requires minimum Nodos {:?} API  minor version {}", name.as_ref().unwrap(), &package_type, min_required_minor).as_str().yellow());
                                }
                            }
                        }
                    }
                }
            }
        }
        let package_type = package_type.unwrap();

        if name.is_none() {
            return Err(InvalidArgumentError { message: "Name is not provided and could not be inferred".to_string() });
        }
        if version.is_none() {
            return Err(InvalidArgumentError { message: "Version is not provided and could not be inferred".to_string() });
        }

        println!("Target platform: {:?}", target_platform);

        let name = name.unwrap();
        let version = version.unwrap() + version_suffix;
        let tag = format!("{}-{}-{}", name, version, target_platform);

        let pb: ProgressBar = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.println(format!("Publishing {}", tag).as_str().yellow().to_string());
        pb.set_message("Preparing release");
        if !Self::is_name_valid(&name) {
            return Err(InvalidArgumentError { message: format!("Name {} is not valid. It should match regex [a-z0-9._]", name) });
        }
        if None == SemVer::parse_from_string(version.as_str()) {
            return Err(InvalidArgumentError { message: format!("Version should be semantic-versioning compatible: {}", version) });
        }
        let workspace = Workspace::get()?;
        let artifact_file_path;
        let temp_dir = tempdir().unwrap();
        if abs_path.is_dir() {
            pb.println("Following files will be included in the release:".yellow().to_string().as_str());
            pb.set_message("Scanning files".to_string());
            let mut files_to_release = vec![];

            let walker = globwalk::GlobWalkerBuilder::from_patterns(&abs_path, &nospub.release_globs)
                .build()
                .expect(format!("Failed to glob dirs: {:?}", nospub.release_globs).as_str());
            for entry in walker {
                let entry = entry.unwrap();
                if entry.file_type().is_dir() {
                    continue;
                }
                let path = entry.path().to_path_buf();
                pb.println(format!("\t{}", path.display()).as_str());
                files_to_release.push(path);
            }

            let host_platform = get_host_platform();
            if target_platform.os != host_platform.os {
                pb.println(format!("Target OS ({}) is different from host OS ({}). Using hosts archive format.", target_platform.os, host_platform.os).yellow().to_string().as_str());
            }

            let mut file_buffer_pairs = vec![];
            for file_path in files_to_release.iter() {
                let mut file = File::open(file_path).expect(format!("Failed to open file: {}", file_path.display()).as_str());
                let mut buffer = Vec::new();
                file.read_to_end(&mut buffer).expect(format!("Failed to read file: {}", file_path.display()).as_str());
                // If this is the manifest file, update the version
                if let Some(m) = &manifest_file {
                    if file_path == m {
                        let mut manifest: serde_json::Value = serde_json::from_slice(&buffer).unwrap();
                        manifest["info"]["id"]["version"] = serde_json::Value::String(version.clone());
                        pb.println(format!("Updated version to {} in manifest file: {}", version.clone(), m.display()).as_str());
                        buffer = serde_json::to_vec_pretty(&manifest).unwrap();
                    }
                }
                file_buffer_pairs.push((file_path.clone(), buffer));
            }

            let archive_file_name = format!("{}.{}", tag, if host_platform.os == "windows" { "zip" } else { "tar.gz" });
            let archive_file_path = temp_dir.path().join(&archive_file_name);
            let archive_file = File::create(&archive_file_path).expect(format!("Failed to create file: {}", archive_file_path.display()).as_str());
            
            #[cfg(target_os = "windows")]
            let mut writer = zip::ZipWriter::new(archive_file);

            #[cfg(target_os = "windows")]
            let options = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            
            #[cfg(unix)]
            let mut writer = tar::Builder::new(flate2::write::GzEncoder::new(archive_file, flate2::Compression::default()));

            for (file_path, buffer) in file_buffer_pairs.iter() {
                pb.set_message(format!("Creating a release: {}", file_path.display()).as_str().to_string());
                #[cfg(target_os = "windows")]
                {
                    writer.start_file(file_path.strip_prefix(&abs_path)
                            .expect(format!("Failed to strip prefix {} from {}", abs_path.display(), file_path.display()).as_str()).to_str()
                            .expect("Failed to convert path to string"), options)
                        .expect(format!("Failed to start file in zip: {}", file_path.display()).as_str());
                    writer.write_all(&buffer).expect(format!("Failed to write to zip: {}", file_path.display()).as_str());
                }
                #[cfg(unix)]
                {
                    let mut header = tar::Header::new_gnu();
                    header.set_path(file_path.strip_prefix(&abs_path)
                        .expect(format!("Failed to strip prefix {} from {}", abs_path.display(), file_path.display()).as_str())
                        .to_str().expect("Failed to convert path to string").to_string()).expect("Failed to set path");
                    header.set_size(buffer.len() as u64);
                    let metadata = file_path.metadata().expect("Failed to get metadata");
                    header.set_mode(metadata.permissions().mode());
                    // Seconds since the Unix epoch
                    if let Ok(modified) = metadata.modified() {
                        header.set_mtime(modified.duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap().as_secs());
                    }
                    header.set_cksum();
                    writer.append(&header, &mut buffer.as_slice()).expect(format!("Failed to append file to tar: {}", file_path.display()).as_str());
                }
            }

            writer.finish().expect(format!("Failed to finish archive: {}", archive_file_path.display()).as_str());
            artifact_file_path = archive_file_path;
        } else {
            pb.set_message(format!("Creating a release: {}", abs_path.display()).as_str().to_string());
            artifact_file_path = abs_path.clone();
        }

        // Create index entry for the release
        let remote = workspace.find_remote(remote_name);
        if remote.is_none() {
            return Err(InvalidArgumentError { message: format!("Remote {} not found", remote_name) });
        }
        let remote = remote.unwrap();

        let now_iso = Utc::now().to_rfc3339();
        let release = PackageReleaseEntry {
            version: version.clone(),
            url: format!("{}/releases/download/{}/{}", remote.url, tag, artifact_file_path.file_name().unwrap().to_str().unwrap()),
            plugin_api_version: match package_type {
                PackageType::Plugin => api_version.clone(),
                _ => None
            },
            subsystem_api_version: match package_type {
                PackageType::Subsystem => api_version,
                _ => None
            },
            release_date: Some(now_iso),
            dependencies,
            category,
            module_tags,
            release_tags: if release_tags.is_empty() { None } else { Some(release_tags.clone()) },
            platform: Some(target_platform.to_string()),
            min_required_api_minor_version: min_required_minor_opt,
        };
        if verbose {
            println!("Release entry: {:?}", release);
        }
        pb.finish_and_clear();

        println!("Adding package {} version {} release entry to remote {}", name, version, remote.name);
        let res = remote.fetch_add(dry_run, verbose, &workspace, &name, vendor, &package_type, release, publisher_name, publisher_email);
        if res.is_err() {
            return Err(GenericError { message: res.err().unwrap() });
        }
        let commit_sha = res.unwrap();

        println!("Uploading release {} on remote {}", format!("{}-{}", name, version), remote.name);
        let res = remote.create_gh_release(dry_run, verbose, &workspace, &commit_sha, &name, &version, &target_platform.to_string(), &tag, vec![artifact_file_path]);
        if res.is_err() {
            return Err(GenericError { message: res.err().unwrap() });
        }
        println!("{}", format!("Release {} on remote {} created successfully", format!("{}-{}", name, version), remote.name).as_str().green().to_string());
        Ok(true)
    }
}

impl Command for PublishCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("publish")
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let path = path::PathBuf::from(args.get_one::<String>("path").unwrap());
        let opt_name = args.get_one::<String>("name");
        let opt_version = args.get_one::<String>("version");
        let version_suffix = args.get_one::<String>("version_suffix").unwrap();
        let package_type: Option<PackageType> = args.get_one::<String>("type").map(|s| serde_json::from_str(format!("\"{}\"", &s).as_str()).unwrap());
        let remote_name = args.get_one::<String>("remote").unwrap();
        let version = if opt_version.is_some() { Some(opt_version.unwrap().clone()) } else { None };
        let name = if opt_name.is_some() { Some(opt_name.unwrap().clone()) } else { None };
        let vendor = args.get_one::<String>("vendor");
        let dry_run = args.get_one::<bool>("dry_run").unwrap();
        let verbose = args.get_one::<bool>("verbose").unwrap();
        let publisher_name = args.get_one::<String>("publisher_name");
        let publisher_email = args.get_one::<String>("publisher_email");
        let release_tags_ref: Vec<&String> = args.get_many::<String>("tag").unwrap_or_default().collect();
        let release_tags: Vec<String> = release_tags_ref.iter().map(|s| s.to_string()).collect();
        let target_platform: Option<&String> = args.get_one::<String>("target_platform");
        self.run_publish(*dry_run, *verbose, &path, name, version, version_suffix, package_type, &remote_name, vendor, publisher_name, publisher_email, &release_tags, target_platform)
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}
