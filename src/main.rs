//! `rmon` is a real-time system profiling tool for Unix-like systems.
//!
//! This crate provides a command-line interface (CLI) that allows users to profile a program's runtime performance
//! and monitor system resources in real-time. It supports subcommands to execute and monitor specific programs.
//!
//! Key features include:
//! - Monitoring program output and system resource usage in parallel.
//! - A real-time terminal-based interface for viewing profiling information.
//! - Configurable profiling duration.
//!
//! Example usage:
//! ```bash
//! rmon run --duration 60 "./my_program"
//! ```

use clap::{Parser, Subcommand};
use ratatui::{
    backend::CrosstermBackend, crossterm::event, layout::{Constraint, Direction, Layout}, widgets::{Block, Borders, Paragraph}, Terminal
};
use std::io::{self, BufRead};
use std::process::{self, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::System;

/// Command-line interface for `rmon`
#[derive(Parser)]
#[command(name = "rmon")]
#[command(about = "A real-time system profiling tool for unix(-like) systems")]
#[command(arg_required_else_help = true)]
struct Cli {
    /// The version of the profiler
    #[arg(
        short = 'v',
        long = "version",
        help = "Prints the version of the rmon profiler"
    )]
    version: bool,

    /// The subcommand to execute
    #[command(subcommand)]
    command: Option<Commands>,
}

/// Subcommands supported by `rmon`
#[derive(Subcommand)]
enum Commands {
    /// Run profiling on a specified program
    Run {
        /// The command to execute and profile
        #[arg(help = "The command to profile")]
        program: String,

        /// Duration for profiling in seconds
        #[arg(
            short,
            long,
            default_value_t = 60,
            help = "Duration for profiling in seconds"
        )]
        duration: u64,
    },
}

fn main() -> Result<(), io::Error> {
    let cli = Cli::parse();

    if cli.version {
        println!("rmon {}", env!("CARGO_PKG_VERSION"));
        println!("A real-time system profiling tool for unix(-like) systems");
        println!("Written by: {}", env!("CARGO_PKG_AUTHORS"));
        process::exit(0);
    }

    match cli.command {
        Some(Commands::Run { program, duration }) => {
            println!("Starting profiling for program: {}", program);

            let sys_info = Arc::new(Mutex::new(System::new_all()));
            let (tx, rx) = mpsc::channel();

            let profiler_thread = {
                let sys_info = Arc::clone(&sys_info);
                thread::spawn(move || {
                    let mut system = System::new_all();
                    let start_time = Instant::now();

                    while start_time.elapsed() < Duration::from_secs(duration) {
                        system.refresh_all();
                        let mut sys_info = sys_info.lock().unwrap();
                        std::mem::swap(&mut *sys_info, &mut system);
                        thread::sleep(Duration::from_secs(1));
                    }
                })
            };

            let child_thread = thread::spawn(move || {
                let mut child = Command::new("sh")
                    .arg("-c")
                    .arg(&program)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .expect("Failed to execute program");

                let stdout = child.stdout.take().expect("Failed to capture stdout");
                let reader = io::BufReader::new(stdout);
                for line in reader.lines() {
                    if let Ok(line) = line {
                        tx.send(line.to_string())
                            .expect("Failed to send child output");
                    }
                }

                let _ = child.wait().expect("Failed to wait on child process");
            });

            let stdout = io::stdout();
            let backend = CrosstermBackend::new(stdout);
            let mut terminal = Terminal::new(backend)?;

            let profiling_end_time = Instant::now() + Duration::from_secs(duration);

            // clear the terminal
            terminal.clear()?;

            while Instant::now() < profiling_end_time {
                terminal.draw(|f| {
                    let chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(f.area());

                    let mut output = String::new();
                    while let Ok(line) = rx.try_recv() {
                        output.push_str(&line);
                        output.push('\n');
                    }
                    let output_widget = Paragraph::new(output).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Program Output"),
                    );
                    f.render_widget(output_widget, chunks[0]);

                    let sys_info = sys_info.lock().unwrap();
                    let mut info = String::new();
                    for process in sys_info.processes() {
                        info.push_str(&format!(
                            "{:?}: {:.2} MB\n",
                            process.1.name(),
                            process.1.memory() as f64 / 1024.0
                        ));
                    }
                    let info_widget = Paragraph::new(info).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Profiling Panel"),
                    );
                    f.render_widget(info_widget, chunks[1]);
                })?;

                if event::poll(std::time::Duration::from_millis(100))? {
                    if let event::Event::Key(key) = event::read()? {
                        if key.code == event::KeyCode::Char('q') {
                            break;
                        }
                    }
                }

                thread::sleep(Duration::from_millis(100));
            }

            println!("Profiling completed.");
        }
        None => {
            eprintln!("No subcommand was provided. Use --help for usage information.");
            process::exit(1);
        }
    }

    Ok(())
}
