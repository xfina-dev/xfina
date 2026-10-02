use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;
use xfina::detect::{Area, Format};
use xfina::models::Schema;

#[derive(Parser, Debug)]
#[command(name = "xfina", about = "CLI to parse financial statements", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Parse a financial statement into JSON, detecting its format
    Parse {
        /// The input file to parse
        file: PathBuf,

        /// Parse as this format instead of detecting one (see `xfina formats`)
        #[arg(long = "as", value_name = "FORMAT", value_parser = parse_format)]
        format: Option<Format>,

        /// Output schema
        #[arg(long, value_enum, default_value = "xfina")]
        schema: SchemaArg,

        /// Password for encrypted PDFs (if required)
        #[arg(short, long)]
        password: Option<String>,

        /// Optional output file path (defaults to <input_file_stem>.json, or
        /// .csv with --csv, in the same directory)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Write a price series as CSV in Tiingo's column layout instead of JSON
        #[arg(long)]
        csv: bool,
    },
    /// Report what a file is, without parsing it
    Detect {
        /// The input file to identify
        file: PathBuf,

        /// Password for encrypted PDFs (if required)
        #[arg(short, long)]
        password: Option<String>,
    },
    /// List the formats this build can read
    Formats {
        /// Only personal statements, or only public data
        #[arg(long, value_enum)]
        area: Option<AreaArg>,
    },
    /// Dump raw text from a PDF or XLS file for development
    Dump {
        /// The input file to dump
        file: PathBuf,

        /// Password for encrypted PDFs (if required)
        #[arg(short, long)]
        password: Option<String>,

        /// Optional output file path (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum AreaArg {
    Personal,
    Public,
}

impl From<AreaArg> for Area {
    fn from(a: AreaArg) -> Self {
        match a {
            AreaArg::Personal => Area::Personal,
            AreaArg::Public => Area::Public,
        }
    }
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum SchemaArg {
    Xfina,
    Rebit,
}

impl From<SchemaArg> for Schema {
    fn from(s: SchemaArg) -> Self {
        match s {
            SchemaArg::Xfina => Schema::Xfina,
            SchemaArg::Rebit => Schema::Rebit,
        }
    }
}

/// Accepts any format id from the registry, so the list of valid values comes
/// from the one table rather than a second copy here.
fn parse_format(value: &str) -> Result<Format, String> {
    Format::from_id(value).ok_or_else(|| {
        let known: Vec<&str> = xfina::formats().iter().map(|f| f.id).collect();
        format!(
            "unknown format '{}'; known formats: {}",
            value,
            known.join(", ")
        )
    })
}

/// Builds the request every subcommand shares.
fn read_request(file: &PathBuf) -> Result<(Vec<u8>, Option<i64>)> {
    let bytes = fs::read(file).with_context(|| format!("Failed to read file: {:?}", file))?;
    let modified = fs::metadata(file)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    Ok((bytes, modified))
}

fn dump_file(file: &PathBuf, password: Option<&str>, output: Option<&PathBuf>) -> Result<()> {
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let content = match ext.as_str() {
        "xls" | "xlsx" | "csv" => dump_spreadsheet(file)?,
        "pdf" => dump_pdf(file, password)?,
        _ => bail!("Unsupported file extension for dump: {}", ext),
    };

    if let Some(out_path) = output {
        fs::write(out_path, content)?;
        println!("Dump saved to {:?}", out_path);
    } else {
        println!("{}", content);
    }

    Ok(())
}

fn dump_spreadsheet(file: &PathBuf) -> Result<String> {
    use calamine::{open_workbook_auto, Data, Reader};
    let mut workbook =
        open_workbook_auto(file).with_context(|| format!("Failed to open workbook: {:?}", file))?;

    let mut result = String::new();
    let sheets = workbook.sheet_names().to_owned();

    for sheet_name in sheets {
        result.push_str(&format!("--- Sheet: {} ---\n", sheet_name));
        if let Ok(range) = workbook.worksheet_range(&sheet_name) {
            for row in range.rows() {
                let row_str = row
                    .iter()
                    .map(|c| match c {
                        Data::String(s) => s.clone(),
                        Data::Float(f) => f.to_string(),
                        Data::Int(i) => i.to_string(),
                        Data::Bool(b) => b.to_string(),
                        Data::DateTime(_) => format!("{:?}", c),
                        Data::DateTimeIso(_) => format!("{:?}", c),
                        Data::DurationIso(_) => format!("{:?}", c),
                        Data::Error(e) => format!("{:?}", e),
                        Data::Empty => String::new(),
                    })
                    .collect::<Vec<_>>()
                    .join("\t");
                result.push_str(&row_str);
                result.push('\n');
            }
        }
    }
    Ok(result)
}

fn dump_pdf(file: &PathBuf, password: Option<&str>) -> Result<String> {
    let bytes = fs::read(file)?;
    let mut doc = pdf_extract::Document::load_mem(&bytes)
        .map_err(|e| anyhow::anyhow!("Failed to load PDF: {:?}", e))?;

    if let Some(pw) = password {
        doc.decrypt(pw)
            .map_err(|e| anyhow::anyhow!("Failed to decrypt PDF: {:?}", e))?;
    } else if doc.is_encrypted() {
        bail!("PDF is encrypted, password required");
    }

    let mut out = String::new();
    {
        let mut plain_text_out = pdf_extract::PlainTextOutput::new(&mut out);
        pdf_extract::output_doc(&doc, &mut plain_text_out)
            .map_err(|e| anyhow::anyhow!("Failed to extract PDF: {:?}", e))?;
    }
    Ok(out)
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Formats { area } => {
            let wanted = area.map(Area::from);
            for info in xfina::formats()
                .into_iter()
                .filter(|f| wanted.is_none_or(|a| f.area == a))
            {
                let locked = if info.password_protected {
                    " locked"
                } else {
                    ""
                };
                let built = if info.enabled { "" } else { "  (not built)" };
                println!(
                    "{:17}  {:8}  {:15}  {:36}  {:5}{}{}",
                    info.id,
                    info.area.as_str(),
                    info.category.as_str(),
                    info.institution,
                    info.extension,
                    locked,
                    built
                );
                // Where the file comes from, indented under its format. Getting
                // hold of the statement is the hard part; reading it is not.
                println!("{:19}{}", "", info.download_url);
                println!("{:19}{}", "", info.download_path);
            }
        }
        Commands::Dump {
            file,
            password,
            output,
        } => {
            dump_file(&file, password.as_deref(), output.as_ref())?;
        }
        Commands::Detect { file, password } => {
            let (bytes, modified) = read_request(&file)?;
            let request = xfina::ParseRequest::new(&bytes)
                .with_password(password.as_deref())
                .with_filename(file.file_name().and_then(|s| s.to_str()))
                .with_modified_timestamp(modified);
            let detection = xfina::detect(&request)?;
            println!("{}", serde_json::to_string_pretty(&detection)?);
        }
        Commands::Parse {
            file,
            format,
            schema,
            password,
            output,
            csv,
        } => {
            let (bytes, modified) = read_request(&file)?;
            let output_path = output.unwrap_or_else(|| {
                let mut path = file.clone();
                path.set_extension(if csv { "csv" } else { "json" });
                path
            });

            let request = xfina::ParseRequest::new(&bytes)
                .with_password(password.as_deref())
                .with_filename(file.file_name().and_then(|s| s.to_str()))
                .with_modified_timestamp(modified)
                .with_format(format);

            let statement = xfina::parse(request)?;
            let rendered = if csv {
                match statement.data.series() {
                    Some(series) => series.to_csv(),
                    None => bail!(
                        "--csv writes a price series; {} is not market data",
                        statement.format
                    ),
                }
            } else {
                statement.to_json_string(schema.into(), true)?
            };

            fs::write(&output_path, rendered)?;
            println!(
                "Parsed {} ({}) to {:?}",
                statement.institution(),
                statement.format,
                output_path
            );
            // A price history is judged by what it covers; say so up front.
            if let Some(series) = statement.data.series() {
                let c = series.coverage();
                match (c.first, c.last) {
                    (Some(first), Some(last)) => println!(
                        "  {} rows of {}, {} to {}, {} gap{}",
                        c.rows,
                        series.headline.as_str(),
                        first,
                        last,
                        c.gaps.len(),
                        if c.gaps.len() == 1 { "" } else { "s" }
                    ),
                    _ => println!("  no rows with a {} value", series.headline.as_str()),
                }
            }
        }
    }

    Ok(())
}
