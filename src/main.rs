use std::{fs, process::ExitCode};

use argh::FromArgs;
use robots_simd::RobotsTxt;

/// parse and check robots.txt files.
#[derive(Debug, FromArgs)]
struct Args {
    /// command to run
    #[argh(subcommand)]
    command: Command,
}

#[derive(Debug, FromArgs)]
#[argh(subcommand)]
enum Command {
    Parse(ParseCommand),
    Check(CheckCommand),
}

/// print the parsed robots.txt structure.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "parse")]
struct ParseCommand {
    /// robots.txt file to parse
    #[argh(positional)]
    file: String,
}

/// check whether a user agent may crawl a path.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "check")]
struct CheckCommand {
    /// robots.txt file to parse
    #[argh(positional)]
    file: String,

    /// crawler product token to check
    #[argh(option)]
    agent: String,

    /// path to check, such as /private/page.html
    #[argh(option)]
    path: String,
}

fn main() -> ExitCode {
    let args: Args = argh::from_env();

    match args.command {
        Command::Parse(command) => parse(command),
        Command::Check(command) => check(command),
    }
}

fn parse(command: ParseCommand) -> ExitCode {
    let Some(input) = read_file(&command.file) else {
        return ExitCode::from(2);
    };
    let robots = RobotsTxt::parse(&input);

    println!("{robots:#?}");
    ExitCode::SUCCESS
}

fn check(command: CheckCommand) -> ExitCode {
    let Some(input) = read_file(&command.file) else {
        return ExitCode::from(2);
    };
    let robots = RobotsTxt::parse(&input);
    let allowed = robots.is_allowed(&command.agent, &command.path);

    println!("{}", if allowed { "allowed" } else { "disallowed" });
    if allowed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn read_file(path: &str) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(input) => Some(input),
        Err(error) => {
            eprintln!("failed to read {path}: {error}");
            None
        }
    }
}
