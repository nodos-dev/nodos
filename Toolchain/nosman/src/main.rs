extern crate clap;
use clap::{Arg, ArgAction, Command};

use std::error::Error;
use colored::Colorize;
use crate::nosman::constants;

mod nosman;

fn print_error(e: &dyn Error) {
    eprintln!("{}", format!("Error: {}", e).as_str().red());
    let mut cause = e.source();
    while let Some(e) = cause {
        eprintln!("{}", format!("Caused by: {}", e).as_str().red());
        cause = e.source();
    }
}

fn main() {
    let mut cmd = Command::new("nosman")
        .version(env!("VERGEN_BUILD_SEMVER"))
        .about("Nodos Package Manager")
        .arg(Arg::new("workspace")
            .help("Directory to the workspace")
            .short('w')
            .long("workspace")
            .default_value(".")
        )
        .subcommand(Command::new("init")
            .about("Initialize a directory as a Nodos workspace.")
        )
        .subcommand(Command::new("deinit")
            .about("Deinitialize a Nodos workspace.")
        )
        .subcommand(Command::new("install")
            .about("Install a module")
            .arg(Arg::new("module").required(true))
            .arg(Arg::new("version").required(false).default_value("latest"))
            .arg(Arg::new("exact")
                .action(ArgAction::SetTrue)
                .help("If not set, version parameter will be interpreted as minimum required version within that minor/patch version.\n\
                If no version 'x' such that 'a.b <= x < a.(b+1)' is found among installed modules, latest such version will be installed.\n\
                If version is set to 'latest' or has no minor component, it will fail.")
                .long("exact")
                .num_args(0)
                .required(false)
            )
            .arg(Arg::new("prefix")
                .help("Folder path relative to out_dir. The module contents will be under this folder. By default, its '<module_name>-<version>'.")
                .long("prefix")
                .required(false)
            )
            .arg(Arg::new("out_dir")
                .help("The directory where the module will be installed")
                .default_value("./Module")
                .long("out-dir")
                .required(false)
            )
        )
        .subcommand(Command::new("remove")
            .about("Remove a module")
            .arg(Arg::new("module").required(true))
            .arg(Arg::new("version").required(true))
        )
        .subcommand(Command::new("rescan")
            .about("Rescan modules and update caches")
            .arg(Arg::new("fetch_index")
                .action(ArgAction::SetTrue)
                .help("Fetch remote module indices before scanning")
                .long("fetch-index")
                .num_args(0)
                .required(false)
            )
        )
        .subcommand(Command::new("list")
            .about("List installed modules")
        )
        .subcommand(Command::new("info")
            .about("Returns information about an installed module in JSON format.\n\
            If no such module is installed, it will return an error.")
            .arg(Arg::new("module").required(true))
            .arg(Arg::new("version").required(true))
            .arg(Arg::new("relaxed")
                .action(ArgAction::SetTrue)
                .help("If set, version parameter will be interpreted as minimum required version within that minor/patch version.\n\
                It will return information about a version 'x' found among installed modules such that 'a.b <= x < a.(b+1)'.")
                .long("relaxed")
                .num_args(0)
                .required(false)
            )
        )
        .subcommand(Command::new("sdk-info")
            .about("Returns information about an installed Nodos SDK under workspace.\n\
            If no such version is found, it will return an error.")
            .arg(Arg::new("version").required(true))
        )
        .subcommand(Command::new("remote")
            .about("Manage remotes.")
            .subcommand(Command::new("add")
                .about("Add a remote")
                .arg(Arg::new("url").required(true))
            )
            .subcommand(Command::new("list")
                .about("List remotes")
            )
            .subcommand(Command::new("remove")
                .about("Remove a remote")
                .arg(Arg::new("url").required(true))
            )
        )
        .subcommand(Command::new("create")
            .about("Interactively create a plugin or subsystem module")
            .arg(Arg::new("type")
                .value_parser(clap::builder::PossibleValuesParser::new(["plugin", "subsystem"]))
                .required(true)
            )
            .arg(Arg::new("name")
                .required(true)
            )
            .arg(Arg::new("language/tool")
                .long("language-tool")
                .short('l')
                .help("Language and tool to use")
                .value_parser(clap::builder::PossibleValuesParser::new(["cpp/cmake"]))
                .default_value("cpp/cmake")
            )
            .arg(Arg::new("output_dir")
                .help("Path to create the module folder in")
                .long("output-dir")
                .short('o')
                .default_value("./Module")
                .required(false)
            )
            .arg(Arg::new("prefix")
                .help("Folder path relative to out_dir. The module contents will be under this folder. By default, its '<module_name>'.")
                .long("prefix")
                .required(false)
            )
            .arg(Arg::new("yes_to_all")
                .action(ArgAction::SetTrue)
                .long("yes-to-all")
                .help("Do not ask for confirmation & use defaults for missing parameters")
                .num_args(0)
                .short('y')
                .required(false)
            )
            .arg(Arg::new("description")
                .help("Description of the module")
                .long("description")
                .default_value("")
                .required(false)
            )
            .arg(Arg::new("dependency")
                .help("Add module dependency. Can be specified multiple times. Format: <module_name>-<version>")
                .long("dependency")
                .short('d')
                .required(false)
                .action(ArgAction::Append)
                .num_args(1)
            )
        )
        .subcommand(Command::new("publish")
            .about("Publish a package")
            .after_help("This command will publish a package to the specified remote.\n\
            Currently, only the git repositories hosted on GitHub can be used to publish.")
            .arg(Arg::new("path")
                .long("path")
                .short('p')
                .help(format!("Path to the root folder of the package (or a file) to be published.\n\
                If not provided, the current directory will be used.\n\
                If the path is a folder and it does not contain a {} file, it will add all files to the release.", constants::PUBLISH_OPTIONS_FILE_NAME))
                .default_value(".")
            )
            .arg(Arg::new("name")
                .long("name")
                .short('n')
                .help("Name of the package. It will be overridden by the module manifest files under <path> if present.\n\
                If the <path> does not contain a module manifest file, this parameter is required."))
            .arg(Arg::new("version")
                .long("version")
                .help("Version of the package. It will be overridden by the module manifest files under <path> if present.\n\
                If the <path> does not contain a module manifest file, this parameter is required.")
            )
            .arg(Arg::new("version_suffix")
                .long("version-suffix")
                .help("Suffix to append to the version of the package.")
                .default_value("")
            )
            .arg(Arg::new("remote")
                .help("Name of the remote to publish to.")
                .default_value("default")
            )
            .arg(Arg::new("type")
                .long("type")
                .short('t')
                .value_parser(clap::builder::PossibleValuesParser::new(["plugin", "subsystem", "nodos", "engine"]))
                .help("Type of the package. It will be overridden by the module manifest files under <path> if present.\n\
                If the <path> does not contain a module manifest file, this parameter is required.")
            )
            .arg(Arg::new("vendor")
                .help("Who is publishing the package?\n\
                Required if the module to be published was not added to the index before.")
                .long("vendor")
            )
            .arg(Arg::new("publisher_name")
                .help("Git name of the publishing agent. If not provided, the name of the current git user will be used.")
                .long("publisher-name")
                .required(false)
            )
            .arg(Arg::new("publisher_email")
                .help("Git email of the publishing agent. If not provided, the email of the current git user will be used.")
                .long("publisher-email")
                .required(false)
            )
            .arg(Arg::new("dry_run")
                .action(ArgAction::SetTrue)
                .long("dry-run")
                .help("Do not actually publish the package, just show what would be done.")
                .num_args(0)
                .required(false)
            )
        )
        .subcommand(Command::new("publish-batch")
            .about("Publish all/changed modules under the git repository.")
            .after_help(format!("This command will publish all/changed modules under the git repository to the specified remote.\n\
            It will use the {} files to compare file changes.", constants::PUBLISH_OPTIONS_FILE_NAME))
            .arg(Arg::new("remote")
                .help("Name of the remote to publish to.")
                .default_value("default")
            )
            .arg(Arg::new("repo_path")
                .long("repo-path")
                .short('r')
                .help("Path to the root folder of the repository. If not provided, the current directory will be used.")
                .default_value(".")
            )
            .arg(Arg::new("compare_with")
                .long("compare-with")
                .short('c')
                .help("Compare current with the given branch, tag or ref.\n\
                If not provided, it will publish all modules found under the provided repo.")
            )
            .arg(Arg::new("version_suffix")
                .long("version-suffix")
                .help("Suffix to append to the version of the modules to be published.")
                .default_value("")
            )
            .arg(Arg::new("vendor")
                .help("Who is publishing the package?\n\
                Required if the module to be published was not added to the index before.")
                .long("vendor")
            )
            .arg(Arg::new("publisher_name")
                .help("Git name of the publishing agent. If not provided, the name of the current git user for the remote will be used.")
                .long("publisher-name")
                .required(false)
            )
            .arg(Arg::new("publisher_email")
                .help("Git email of the publishing agent. If not provided, the email of the current git user for the remote will be used.")
                .long("publisher-email")
                .required(false)
            )
            .arg(Arg::new("dry_run")
                .action(ArgAction::SetTrue)
                .long("dry-run")
                .help("Do not actually publish the package, just show what would be done.")
                .num_args(0)
                .required(false)
            )
        );

    let help_str = cmd.render_help();
    let matches = cmd.get_matches();

    let mut matched = false;
    for command in nosman::command::commands().iter() {
        match command.matched_args(&matches) {
            Some(command_args) => {
                nosman::workspace::set_current_root(dunce::canonicalize(std::path::PathBuf::from(matches.get_one::<String>("workspace").unwrap())).unwrap());
                if (*command).needs_workspace() {
                    if !nosman::workspace::current_root().unwrap().join(".nosman").exists() {
                        eprintln!("No workspace found in {:?}", matches.get_one::<String>("workspace").unwrap());
                        std::process::exit(1);
                    }
                }
                match (*command).run(command_args) {
                    Ok(_) => {
                        // nothing
                    },
                    Err(e) => {
                        print_error(&e);
                        std::process::exit(1);
                    }
                };
                matched = true;
                break;
            },
            None => continue,
        };
    }

    if !matched {
        println!("{}", help_str.ansi());
        std::process::exit(1);
    }

    std::process::exit(0);
}

