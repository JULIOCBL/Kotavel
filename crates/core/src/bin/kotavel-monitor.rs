use serde_json::Value;
use std::{
    env, fs,
    io::{self, BufRead, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    path::{Path, PathBuf},
    time::Duration,
};

struct Options {
    project: PathBuf,
    pid: Option<u32>,
}

fn main() {
    match run(env::args().skip(1)) {
        Ok(()) => {}
        Err(error) => {
            eprintln!("ERROR: {error}");
            std::process::exit(1);
        }
    }
}

fn run(arguments: impl IntoIterator<Item = String>) -> Result<(), String> {
    let Some(options) = parse_arguments(arguments)? else {
        println!("Uso: kotavel-monitor [--project RUTA] [--pid PID]");
        return Ok(());
    };
    let project = resolve_project(options.project)?;
    let stream = connect(&project, options.pid)?;
    stream
        .set_read_timeout(None)
        .map_err(|error| format!("No se pudo configurar la conexión: {error}"))?;
    println!(
        "Monitor conectado a {}. Presiona Ctrl+C para desconectarte.",
        project.display()
    );
    io::stdout().flush().map_err(|error| error.to_string())?;

    let mut reader = io::BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .map_err(|error| format!("No se pudo leer el stream de métricas: {error}"))?;
        if bytes == 0 {
            break;
        }
        let event: Value = serde_json::from_str(&line)
            .map_err(|error| format!("Kotavel envió un evento de métricas inválido: {error}"))?;
        render_event(&event, &mut io::stdout()).map_err(|error| error.to_string())?;
        io::stdout().flush().map_err(|error| error.to_string())?;
    }
    println!("La aplicación Kotavel se desconectó.");
    Ok(())
}

fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Option<Options>, String> {
    let mut project = env::current_dir().map_err(|error| error.to_string())?;
    let mut pid = None;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "--help" || argument == "-h" {
            return Ok(None);
        }
        let (option, inline_value) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(name, value)| {
                (name, Some(value))
            });
        match option {
            "--project" => {
                let value = inline_value
                    .map(str::to_owned)
                    .or_else(|| arguments.next())
                    .ok_or_else(|| "--project necesita una ruta".to_owned())?;
                project = PathBuf::from(value);
            }
            "--pid" => {
                let value = inline_value
                    .map(str::to_owned)
                    .or_else(|| arguments.next())
                    .ok_or_else(|| "--pid necesita un PID".to_owned())?;
                let parsed = value
                    .parse::<u32>()
                    .map_err(|_| "--pid debe ser un entero positivo".to_owned())?;
                if parsed == 0 {
                    return Err("--pid debe ser un entero positivo".into());
                }
                pid = Some(parsed);
            }
            _ => return Err(format!("opción desconocida: {argument}")),
        }
    }
    Ok(Some(Options { project, pid }))
}

fn resolve_project(project: PathBuf) -> Result<PathBuf, String> {
    if project.is_absolute() {
        Ok(project)
    } else {
        env::current_dir()
            .map(|directory| directory.join(project))
            .map_err(|error| error.to_string())
    }
}

fn connect(project: &Path, pid: Option<u32>) -> Result<TcpStream, String> {
    let directory = project.join("storage/framework/metrics");
    let entries = fs::read_dir(&directory).map_err(|_| no_metrics_message(project, pid))?;
    let mut candidates = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                return false;
            };
            let Some(value) = name
                .strip_prefix("kotavel-")
                .and_then(|name| name.strip_suffix(".endpoint"))
            else {
                return false;
            };
            value.parse::<u32>().is_ok_and(|candidate| {
                candidate > 0 && pid.is_none_or(|requested| requested == candidate)
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    candidates.reverse();

    for endpoint in candidates {
        let Ok(contents) = fs::read_to_string(&endpoint) else {
            continue;
        };
        let Ok(port) = contents.trim().parse::<u16>() else {
            continue;
        };
        if port == 0 {
            continue;
        }
        if let Ok(stream) = TcpStream::connect_timeout(
            &SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into(),
            Duration::from_secs(2),
        ) {
            return Ok(stream);
        }
    }
    Err(no_metrics_message(project, pid))
}

fn no_metrics_message(project: &Path, pid: Option<u32>) -> String {
    match pid {
        Some(pid) => format!(
            "No hay métricas activas para el proceso {pid} en {}.",
            project.display()
        ),
        None => format!(
            "No hay una aplicación Kotavel activa en {}. Inicia primero `kotavel run`.",
            project.display()
        ),
    }
}

fn render_event(event: &Value, output: &mut impl Write) -> io::Result<()> {
    match event.get("type").and_then(Value::as_str) {
        Some("startup") => {
            let environment = numeric_field(event, "environment_ms")?;
            let configuration = numeric_field(event, "configuration_ms")?;
            let settings = numeric_field(event, "settings_ms")?;
            let application = numeric_field(event, "application_boot_ms")?;
            let total = numeric_field(event, "total_ms")?;
            writeln!(
                output,
                "[Kotavel] Arranque: entorno {environment:.2} ms | configuración {configuration:.2} ms | opciones {settings:.2} ms | aplicación {application:.2} ms | total {total:.2} ms"
            )
        }
        Some("sample") => {
            let cpu = match event.get("cpu_percent") {
                None | Some(Value::Null) => "calculando".to_owned(),
                Some(value) => format!("{:.1} %", value.as_f64().ok_or_else(invalid_event)?),
            };
            let memory = numeric_field(event, "resident_memory_bytes")? / (1024.0 * 1024.0);
            writeln!(
                output,
                "[Kotavel] CPU: {cpu} | RAM residente: {memory:.1} MiB"
            )
        }
        _ => Err(invalid_event()),
    }
}

fn numeric_field(event: &Value, name: &str) -> io::Result<f64> {
    event
        .get(name)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(invalid_event)
}

fn invalid_event() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Kotavel envió un evento de métricas desconocido o incompleto",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_project_and_pid_flags_with_both_forms() {
        let options =
            parse_arguments(["--project".into(), "/tmp/kotavel".into(), "--pid=42".into()])
                .unwrap()
                .unwrap();
        assert_eq!(options.project, PathBuf::from("/tmp/kotavel"));
        assert_eq!(options.pid, Some(42));
    }

    #[test]
    fn rejects_invalid_pid_and_unknown_options() {
        assert!(parse_arguments(["--pid=0".into()]).is_err());
        assert!(parse_arguments(["--pid=nope".into()]).is_err());
        assert!(parse_arguments(["--unexpected".into()]).is_err());
    }

    #[test]
    fn renders_startup_sample_and_null_cpu_events() {
        let startup = serde_json::json!({
            "type":"startup", "environment_ms":1.0, "configuration_ms":2.0,
            "settings_ms":3.0, "application_boot_ms":4.0, "total_ms":10.0
        });
        let mut output = Vec::new();
        render_event(&startup, &mut output).unwrap();
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("total 10.00 ms")
        );

        let sample = serde_json::json!({"type":"sample", "cpu_percent":null,
            "resident_memory_bytes":1048576});
        let mut output = Vec::new();
        render_event(&sample, &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("CPU: calculando"));
        assert!(output.contains("RAM residente: 1.0 MiB"));
    }

    #[test]
    fn connects_only_to_the_requested_loopback_endpoint() {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project = env::temp_dir().join(format!("kotavel-monitor-{unique}"));
        let directory = project.join("storage/framework/metrics");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("kotavel-321.endpoint"), port.to_string()).unwrap();
        fs::write(directory.join("kotavel-999.endpoint"), "not-a-port").unwrap();

        let stream = connect(&project, Some(321)).unwrap();
        assert_eq!(stream.peer_addr().unwrap().ip(), Ipv4Addr::LOCALHOST);
        assert_eq!(
            listener.accept().unwrap().0.peer_addr().unwrap().ip(),
            Ipv4Addr::LOCALHOST
        );

        drop(stream);
        fs::remove_dir_all(project).unwrap();
    }
}
