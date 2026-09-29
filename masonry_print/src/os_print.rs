// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Sending PDF documents to the operating system's printing system.
//!
//! - **Linux and macOS**: the CUPS `lp` command, which accepts PDF directly.
//!   Printers are listed with `lpstat`.
//! - **Windows**: PowerShell. If [SumatraPDF](https://www.sumatrapdfreader.org) is installed it
//!   is used for silent printing; otherwise the document is printed with the `Print`/`PrintTo`
//!   verb of the registered PDF application (which must support it). Printers are listed with
//!   `Get-Printer`.
//!
//! No native print dialog is shown. Paths and printer names are passed to PowerShell through
//! environment variables, never interpolated into the script.

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The operating system family, which determines the print command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetOs {
    /// Windows.
    Windows,
    /// macOS.
    MacOs,
    /// Linux and other Unix systems with CUPS.
    Unix,
}

impl TargetOs {
    /// The operating system this program runs on.
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Unix
        }
    }
}

/// Two-sided printing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duplex {
    /// Print on one side.
    OneSided,
    /// Print on both sides, flipping on the long edge (portrait documents).
    LongEdge,
    /// Print on both sides, flipping on the short edge (landscape documents).
    ShortEdge,
}

/// Options for printing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintOptions {
    /// Printer name. `None` uses the default printer.
    pub printer: Option<String>,
    /// Number of copies.
    pub copies: u32,
    /// Two-sided printing (CUPS only).
    pub duplex: Option<Duplex>,
    /// Media (paper) name, e.g. `"A4"` or `"Letter"` (CUPS only).
    pub media: Option<String>,
    /// Scale the document to fit the paper (CUPS only).
    pub fit_to_page: bool,
    /// Job title.
    pub title: Option<String>,
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            printer: None,
            copies: 1,
            duplex: None,
            media: None,
            fit_to_page: false,
            title: None,
        }
    }
}

/// A command to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintCommand {
    /// The program.
    pub program: String,
    /// Arguments.
    pub args: Vec<OsString>,
    /// Additional environment variables.
    pub env: Vec<(String, OsString)>,
}

impl fmt::Display for PrintCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.program)?;
        for arg in &self.args {
            write!(f, " {}", arg.to_string_lossy())?;
        }
        Ok(())
    }
}

/// Environment variable holding the file path for the Windows print script.
const FILE_VAR: &str = "MASONRY_PRINT_FILE";
/// Environment variable holding the printer name for the Windows print script.
const PRINTER_VAR: &str = "MASONRY_PRINT_PRINTER";
/// Environment variable holding the number of copies for the Windows print script.
const COPIES_VAR: &str = "MASONRY_PRINT_COPIES";

const WINDOWS_PRINT_SCRIPT: &str = "\
$ErrorActionPreference = 'Stop'
$file = $env:MASONRY_PRINT_FILE
$printer = $env:MASONRY_PRINT_PRINTER
$copies = [int]$env:MASONRY_PRINT_COPIES
$sumatra = @(\"$env:LOCALAPPDATA\\SumatraPDF\\SumatraPDF.exe\", \"$env:ProgramFiles\\SumatraPDF\\SumatraPDF.exe\") | Where-Object { Test-Path $_ } | Select-Object -First 1
if ($sumatra) {
  $target = if ($printer) { @('-print-to', $printer) } else { @('-print-to-default') }
  $settings = @('-print-settings', \"${copies}x\")
  & $sumatra @target @settings -silent $file
  exit $LASTEXITCODE
}
for ($i = 0; $i -lt $copies; $i++) {
  if ($printer) {
    Start-Process -FilePath $file -Verb PrintTo -ArgumentList ('\"' + $printer + '\"') -Wait
  } else {
    Start-Process -FilePath $file -Verb Print -Wait
  }
}
";

/// Builds the command that prints `pdf` on the given operating system.
pub fn build_print_command(os: TargetOs, pdf: &Path, options: &PrintOptions) -> PrintCommand {
    match os {
        TargetOs::Unix | TargetOs::MacOs => {
            let mut args: Vec<OsString> = Vec::new();
            if let Some(printer) = &options.printer {
                args.push("-d".into());
                args.push(printer.into());
            }
            if options.copies > 1 {
                args.push("-n".into());
                args.push(options.copies.to_string().into());
            }
            if let Some(title) = &options.title {
                args.push("-t".into());
                args.push(title.into());
            }
            if let Some(duplex) = options.duplex {
                let sides = match duplex {
                    Duplex::OneSided => "one-sided",
                    Duplex::LongEdge => "two-sided-long-edge",
                    Duplex::ShortEdge => "two-sided-short-edge",
                };
                args.push("-o".into());
                args.push(format!("sides={sides}").into());
            }
            if let Some(media) = &options.media {
                args.push("-o".into());
                args.push(format!("media={media}").into());
            }
            if options.fit_to_page {
                args.push("-o".into());
                args.push("fit-to-page".into());
            }
            args.push("--".into());
            args.push(pdf.into());
            PrintCommand {
                program: "lp".into(),
                args,
                env: Vec::new(),
            }
        }
        TargetOs::Windows => PrintCommand {
            program: "powershell.exe".into(),
            args: vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-Command".into(),
                WINDOWS_PRINT_SCRIPT.into(),
            ],
            env: vec![
                (FILE_VAR.into(), pdf.into()),
                (
                    PRINTER_VAR.into(),
                    options.printer.clone().unwrap_or_default().into(),
                ),
                (COPIES_VAR.into(), options.copies.max(1).to_string().into()),
            ],
        },
    }
}

/// Builds the command that lists printers.
pub fn build_list_printers_command(os: TargetOs) -> PrintCommand {
    match os {
        TargetOs::Unix | TargetOs::MacOs => PrintCommand {
            program: "lpstat".into(),
            args: vec!["-d".into(), "-e".into()],
            env: Vec::new(),
        },
        TargetOs::Windows => PrintCommand {
            program: "powershell.exe".into(),
            args: vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                "$d = (Get-CimInstance Win32_Printer | Where-Object Default).Name; \
                 Get-Printer | ForEach-Object { if ($_.Name -eq $d) { '*' + $_.Name } else { $_.Name } }"
                    .into(),
            ],
            env: Vec::new(),
        },
    }
}

/// Runs commands. Implement this to intercept printing, e.g. in tests.
pub trait CommandRunner {
    /// Runs the command to completion.
    fn run(&self, command: &PrintCommand) -> io::Result<Output>;
}

/// Runs commands as child processes.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, command: &PrintCommand) -> io::Result<Output> {
        let mut cmd = Command::new(&command.program);
        cmd.args(&command.args);
        for (key, value) in &command.env {
            cmd.env(key, value);
        }
        cmd.output()
    }
}

/// Errors when printing.
#[derive(Debug)]
pub enum PrintError {
    /// The print command could not be started (e.g. CUPS is not installed).
    NoPrintSystem(String),
    /// The print command failed.
    Failed {
        /// The command that was run.
        command: String,
        /// Its exit code, if any.
        code: Option<i32>,
        /// Its error output.
        stderr: String,
    },
    /// An I/O error (e.g. writing a temporary file).
    Io(io::Error),
}

impl fmt::Display for PrintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPrintSystem(e) => write!(f, "no printing system available: {e}"),
            Self::Failed {
                command,
                code,
                stderr,
            } => write!(f, "`{command}` failed ({code:?}): {}", stderr.trim()),
            Self::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for PrintError {}

impl From<io::Error> for PrintError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// The result of a submitted print job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintReport {
    /// The command that was run.
    pub command: String,
    /// The job id reported by the printing system, if any.
    pub job_id: Option<String>,
}

/// A printer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Printer {
    /// Printer name, to use in [`PrintOptions::printer`].
    pub name: String,
    /// Whether this is the default printer.
    pub is_default: bool,
}

fn run(runner: &dyn CommandRunner, command: &PrintCommand) -> Result<String, PrintError> {
    let output = runner
        .run(command)
        .map_err(|e| PrintError::NoPrintSystem(format!("{}: {e}", command.program)))?;
    if !output.status.success() {
        return Err(PrintError::Failed {
            command: command.program.clone(),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parses the job id from the output of `lp` ("request id is printer-42 (1 file(s))").
pub fn parse_lp_job_id(stdout: &str) -> Option<String> {
    let rest = stdout.split("request id is ").nth(1)?;
    rest.split_whitespace().next().map(str::to_string)
}

/// Parses the output of `lpstat -d -e`.
pub fn parse_lpstat(stdout: &str) -> Vec<Printer> {
    let default = stdout
        .lines()
        .find_map(|line| line.strip_prefix("system default destination: "))
        .map(str::trim);
    stdout
        .lines()
        .filter(|line| !line.contains(':') && !line.trim().is_empty())
        .map(|line| {
            let name = line.trim().to_string();
            Printer {
                is_default: Some(name.as_str()) == default,
                name,
            }
        })
        .collect()
}

/// Parses the output of the Windows printer listing (default printer prefixed with `*`).
pub fn parse_windows_printers(stdout: &str) -> Vec<Printer> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| match line.strip_prefix('*') {
            Some(name) => Printer {
                name: name.to_string(),
                is_default: true,
            },
            None => Printer {
                name: line.to_string(),
                is_default: false,
            },
        })
        .collect()
}

/// Prints a PDF file with the given runner and operating system.
pub fn print_pdf_with(
    runner: &dyn CommandRunner,
    os: TargetOs,
    pdf: &Path,
    options: &PrintOptions,
) -> Result<PrintReport, PrintError> {
    let command = build_print_command(os, pdf, options);
    let stdout = run(runner, &command)?;
    Ok(PrintReport {
        command: command.program.clone(),
        job_id: parse_lp_job_id(&stdout),
    })
}

/// Prints a PDF file.
pub fn print_pdf(pdf: &Path, options: &PrintOptions) -> Result<PrintReport, PrintError> {
    print_pdf_with(&SystemRunner, TargetOs::current(), pdf, options)
}

/// Writes PDF bytes to a temporary file and prints it.
///
/// The file is left in the temporary directory, since print spoolers may read it after the
/// command returns.
pub fn print_pdf_bytes(bytes: &[u8], options: &PrintOptions) -> Result<PrintReport, PrintError> {
    let path = temp_pdf_path("print");
    std::fs::write(&path, bytes)?;
    print_pdf(&path, options)
}

/// Lists the available printers.
pub fn list_printers() -> Result<Vec<Printer>, PrintError> {
    list_printers_with(&SystemRunner, TargetOs::current())
}

/// Lists the available printers with the given runner and operating system.
pub fn list_printers_with(
    runner: &dyn CommandRunner,
    os: TargetOs,
) -> Result<Vec<Printer>, PrintError> {
    let stdout = run(runner, &build_list_printers_command(os))?;
    Ok(match os {
        TargetOs::Windows => parse_windows_printers(&stdout),
        _ => parse_lpstat(&stdout),
    })
}

/// A unique path for a PDF file in the temporary directory.
pub fn temp_pdf_path(prefix: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{}-{n}.pdf", std::process::id()))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::process::ExitStatus;

    use super::*;

    fn args(command: &PrintCommand) -> Vec<String> {
        command
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn cups_command() {
        let options = PrintOptions {
            printer: Some("Office Printer".into()),
            copies: 2,
            duplex: Some(Duplex::LongEdge),
            media: Some("A4".into()),
            fit_to_page: true,
            title: Some("Invoice".into()),
        };
        let command = build_print_command(TargetOs::Unix, Path::new("/tmp/a b.pdf"), &options);
        assert_eq!(command.program, "lp");
        assert_eq!(
            args(&command),
            [
                "-d",
                "Office Printer",
                "-n",
                "2",
                "-t",
                "Invoice",
                "-o",
                "sides=two-sided-long-edge",
                "-o",
                "media=A4",
                "-o",
                "fit-to-page",
                "--",
                "/tmp/a b.pdf"
            ]
        );
        let default = build_print_command(
            TargetOs::MacOs,
            Path::new("x.pdf"),
            &PrintOptions::default(),
        );
        assert_eq!(args(&default), ["--", "x.pdf"]);
    }

    #[test]
    fn windows_command_passes_values_through_environment() {
        let options = PrintOptions {
            printer: Some("Evil'; Remove-Item -Recurse C:\\ #".into()),
            copies: 3,
            ..PrintOptions::default()
        };
        let command = build_print_command(TargetOs::Windows, Path::new("C:\\a'b.pdf"), &options);
        assert_eq!(command.program, "powershell.exe");
        let script = command.args.last().unwrap().to_string_lossy().into_owned();
        assert!(
            !script.contains("Evil"),
            "printer name must not be in the script"
        );
        assert!(!script.contains("a'b"), "path must not be in the script");
        assert!(
            command
                .env
                .contains(&(FILE_VAR.into(), "C:\\a'b.pdf".into()))
        );
        assert!(command.env.contains(&(COPIES_VAR.into(), "3".into())));
    }

    #[test]
    fn parsing() {
        assert_eq!(
            parse_lp_job_id("request id is Office-42 (1 file(s))\n"),
            Some("Office-42".into())
        );
        let printers = parse_lpstat("system default destination: Office\nHome\nOffice\n");
        assert_eq!(
            printers,
            vec![
                Printer {
                    name: "Home".into(),
                    is_default: false
                },
                Printer {
                    name: "Office".into(),
                    is_default: true
                }
            ]
        );
        let printers = parse_windows_printers("Microsoft Print to PDF\r\n*HP LaserJet\r\n");
        assert_eq!(printers.len(), 2);
        assert!(printers[1].is_default);
        assert_eq!(printers[1].name, "HP LaserJet");
    }

    struct FakeRunner {
        commands: RefCell<Vec<PrintCommand>>,
        stdout: &'static str,
        success: bool,
    }

    #[cfg(unix)]
    fn status(success: bool) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(if success { 0 } else { 1 << 8 })
    }

    #[cfg(windows)]
    fn status(success: bool) -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(if success { 0 } else { 1 })
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, command: &PrintCommand) -> io::Result<Output> {
            self.commands.borrow_mut().push(command.clone());
            Ok(Output {
                status: status(self.success),
                stdout: self.stdout.as_bytes().to_vec(),
                stderr: b"lp: The printer or class does not exist.".to_vec(),
            })
        }
    }

    #[test]
    fn printing_with_a_runner() {
        let runner = FakeRunner {
            commands: RefCell::new(Vec::new()),
            stdout: "request id is Office-7 (1 file(s))",
            success: true,
        };
        let report = print_pdf_with(
            &runner,
            TargetOs::Unix,
            Path::new("doc.pdf"),
            &PrintOptions::default(),
        )
        .unwrap();
        assert_eq!(report.job_id.as_deref(), Some("Office-7"));
        assert_eq!(runner.commands.borrow().len(), 1);

        let failing = FakeRunner {
            commands: RefCell::new(Vec::new()),
            stdout: "",
            success: false,
        };
        let error = print_pdf_with(
            &failing,
            TargetOs::Unix,
            Path::new("doc.pdf"),
            &PrintOptions::default(),
        )
        .unwrap_err();
        assert!(matches!(error, PrintError::Failed { .. }), "{error}");
    }
}
