use std::fs::File;
use std::io::{self, Write};
use regex::Regex;
use walkdir::WalkDir;
use std::path::PathBuf;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use notify_debouncer_full::new_debouncer;
use notify_debouncer_full::notify::{RecursiveMode, Watcher};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "ts_endpoint_generator")]
struct Opt {
    // Input directory containing TypeScript files
    #[arg(long)]
    input: PathBuf,
    // Output file for generated endpoints
    #[arg(long, default_value = "output.ts")]
    output: PathBuf,
    // Watch for file changes
    #[arg(long)]
    watch: bool,
}

fn main() -> io::Result<()> {

    let opt = Opt::parse();

    let mut stdout = StandardStream::stdout(ColorChoice::Auto);

    if opt.watch {
        use std::sync::mpsc::channel;

        let (tx, rx) = channel();
        let mut debouncer = new_debouncer(Duration::from_secs(1), None, tx).unwrap();
        debouncer.watcher().watch(&opt.input, RecursiveMode::Recursive).unwrap();

        let mut color_spec = ColorSpec::new();
        color_spec.set_bold(true).set_fg(Some(Color::Yellow));
        stdout.set_color(&color_spec)?;
        write!(&mut stdout, "!")?;
        stdout.reset()?;
        writeln!(
            &mut stdout,
            " Watching for changes in {:?}...",
            &opt.input
        )?;

        if let Err(e) = process_files(&opt) {
            print_error(&mut stdout, &format!("Error during initial processing: {:?}", e))?;
        }

        for result in rx {
            match result {
                Ok(_events) => {
                    let mut color_spec = ColorSpec::new();
                    color_spec.set_bold(true).set_fg(Some(Color::Yellow));
                    stdout.set_color(&color_spec)?;
                    writeln!(&mut stdout, "! Detected changes")?;
                    stdout.reset()?;
                    if let Err(e) = process_files(&opt) {
                        print_error(&mut stdout, &format!("{:?}", e))?;
                    }
                }
                Err(errors) => {
                    for error in errors {
                        print_error(&mut stdout, &format!("{:?}", error))?;
                    }
                }
            }
        }
    } else {
        if let Err(e) = process_files(&opt) {
            print_error(&mut stdout, &format!("{:?}", e))?;
            std::process::exit(1);
        }
    }

    Ok(())
}

fn process_files(opt: &Opt) -> io::Result<()> {
    let start_time = Instant::now();
    let backend_api_re = Regex::new(r#"@backendAPI\(\s*"(?P<group_name>[^"]+)"\s*\)"#).unwrap();
    let class_re = Regex::new(r"class\s+(?P<class_name>\w+)\s*").unwrap();
    let method_re = Regex::new(r#"@route\(\s*\)\s+async\s+(?P<method_name>\w+)\s*\((?P<params>[^)]*)\)"#).unwrap();
    let mut endpoints: HashMap<String, HashMap<String, (String, String, String)>> = HashMap::new();
    let mut method_sources: HashMap<(String, String), Vec<String>> = HashMap::new();

    for entry in WalkDir::new(&opt.input) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Error reading directory entry: {}", e);
                continue;
            }
        };
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("ts") {
            let contents = std::fs::read_to_string(path)?;

            let backend_api_cap = backend_api_re.captures(&contents);
            let group_name = if let Some(cap) = backend_api_cap {
                cap["group_name"].to_string()
            } else {
                continue;
            };

            let class_cap = class_re.captures(&contents);
            let class_name = if let Some(cap) = class_cap {
                cap["class_name"].to_string()
            } else {
                continue;
            };

            for cap in method_re.captures_iter(&contents) {
                let method_name = cap["method_name"].to_string();
                let params = &cap["params"];
                let params_list: Vec<&str> = params.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
                let mut param_names = Vec::new();
                let mut param_defs = Vec::new();

                for param in params_list {
                    let parts: Vec<&str> = param.split(':').map(|s| s.trim()).collect();
                    if parts.len() == 2 {
                        param_names.push(parts[0].to_string());
                        param_defs.push(format!("{}: {}", parts[0], parts[1]));
                    }
                }

                let param_defs_str = param_defs.join(", ");
                let param_names_str = param_names.join(", ");
                let full_route_name = format!("{}-{}", group_name, method_name);
                let group_methods = endpoints.entry(group_name.clone()).or_insert_with(HashMap::new);

                if group_methods.contains_key(&method_name) {
                    let sources = method_sources.get(&(group_name.clone(), method_name.clone())).unwrap();
                    let mut error_message = format!("Duplicate method name '{}' found in group '{}':\n", method_name, group_name);
                    for source in sources {
                        error_message.push_str(&format!("- {}\n", source));
                    }
                    error_message.push_str(&format!("- {}", class_name));
                    return Err(io::Error::new(io::ErrorKind::Other, error_message));
                } else {
                    group_methods.insert(method_name.clone(), (param_defs_str, param_names_str, full_route_name));
                    method_sources.entry((group_name.clone(), method_name.clone())).or_insert_with(Vec::new).push(class_name.clone());
                }
            }
        }
    }

    let mut file = File::create(&opt.output)?;
    writeln!(file, "import {{ ipcRenderer }} from \"electron\";\n")?;
    writeln!(file, "export const api = {{")?;

    for (group_name, methods) in endpoints {
        writeln!(file, "  {}: {{", group_name)?;
        for (method_name, (param_defs_str, param_names_str, full_route_name)) in methods {
            writeln!(file, "    {}: async ({}) => {{", method_name, param_defs_str)?;
            writeln!(file, "      return await ipcRenderer.invoke(\"{}\", {});", full_route_name, param_names_str)?;
            writeln!(file, "    }},")?;
        }
        writeln!(file, "  }},")?;
    }
    writeln!(file, "}};")?;

    let duration = start_time.elapsed();
    let mut stdout = StandardStream::stdout(ColorChoice::Auto);

    if duration.as_secs() >= 1 {
        let mut color_spec = ColorSpec::new();
        color_spec.set_bold(true).set_fg(Some(Color::Green));
        stdout.set_color(&color_spec)?;
        writeln!(&mut stdout, "✓ Finished in {} seconds.", duration.as_secs())?;
        stdout.reset()?;
    } else {
        let mut color_spec = ColorSpec::new();
        color_spec.set_bold(true).set_fg(Some(Color::Green));
        stdout.set_color(&color_spec)?;
        writeln!(&mut stdout, "✓ Finished in {} ms.", duration.as_millis())?;
        stdout.reset()?;
    }

    Ok(())
}

fn print_error(stdout: &mut StandardStream, msg: &str) -> io::Result<()> {
    let mut color_spec = ColorSpec::new();
    color_spec.set_bold(true).set_fg(Some(Color::Red));
    stdout.set_color(&color_spec)?;
    writeln!(stdout, "❌ {}", msg)?;
    stdout.reset()?;
    Ok(())
}
