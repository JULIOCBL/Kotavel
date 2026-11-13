#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;
use std::{
    env, fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
#[cfg(unix)]
use sysinfo::{Pid, Process, ProcessStatus, ProcessesToUpdate, Signal, System};

struct Options {
    project: PathBuf,
    pid: Option<u32>,
}

struct Instance {
    endpoint: PathBuf,
    pid: u32,
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
        println!("Uso: kotavel-stop [--project RUTA] [--pid PID]");
        return Ok(());
    };
    let project = resolve_project(options.project)?;
    let instances = find_instances(&project, options.pid)?;
    #[cfg(unix)]
    let mut system = System::new();

    let mut stopped = Vec::new();
    let mut failures = Vec::new();
    for instance in instances {
        match stop_instance(
            #[cfg(unix)]
            &mut system,
            &instance,
            &project,
        ) {
            Ok(forced) => stopped.push((instance.pid, forced)),
            Err(error) => failures.push(format!("{}: {error}", instance.pid)),
        }
    }
    for (pid, forced) in stopped {
        let suffix = if forced {
            " (cierre forzado después de esperar 10 segundos)"
        } else {
            ""
        };
        println!("Aplicación Kotavel detenida (PID {pid}){suffix}.");
    }
    if !failures.is_empty() {
        return Err(format!(
            "No se pudieron detener todas las instancias: {}",
            failures.join("; ")
        ));
    }
    Ok(())
}

fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Option<Options>, String> {
    let mut project = env::current_dir().map_err(|error| error.to_string())?;
    let mut pid = None;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if matches!(argument.as_str(), "--help" | "-h") {
            return Ok(None);
        }
        let (option, inline_value) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(name, value)| {
                (name, Some(value))
            });
        match option {
            "--project" => {
                project = PathBuf::from(
                    inline_value
                        .map(str::to_owned)
                        .or_else(|| arguments.next())
                        .ok_or_else(|| "--project necesita una ruta".to_owned())?,
                );
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
    let project = if project.is_absolute() {
        project
    } else {
        env::current_dir()
            .map_err(|error| error.to_string())?
            .join(project)
    };
    project.canonicalize().map_err(|error| {
        format!(
            "No se pudo resolver el proyecto {}: {error}",
            project.display()
        )
    })
}

fn find_instances(project: &Path, requested_pid: Option<u32>) -> Result<Vec<Instance>, String> {
    let directory = project.join("storage/framework/metrics");
    let entries = fs::read_dir(&directory).map_err(|_| {
        format!(
            "No hay una instancia activa de Kotavel para {}.",
            project.display()
        )
    })?;
    #[cfg(unix)]
    let mut system = System::new();
    let mut candidates = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let endpoint = entry.path();
        let Some(pid) = process_id(&endpoint) else {
            continue;
        };
        if requested_pid.is_some_and(|requested| requested != pid) || !valid_endpoint(&endpoint) {
            continue;
        }
        #[cfg(unix)]
        {
            let process = Pid::from(pid as usize);
            system.refresh_processes(ProcessesToUpdate::Some(&[process]), true);
            if let Some(process_info) = system.process(process) {
                if process_belongs_to_project(process_info, project) {
                    candidates.push(Instance { endpoint, pid });
                }
            }
        }
        #[cfg(windows)]
        if process_belongs_to_project(pid, project) {
            candidates.push(Instance { endpoint, pid });
        }
    }
    candidates.sort_by_key(|instance| instance.pid);
    if candidates.is_empty() {
        return Err(match requested_pid {
            Some(pid) => format!(
                "No se encontró una instancia activa de Kotavel con PID {pid} en este proyecto."
            ),
            None => "No se encontró una instancia activa de Kotavel en este proyecto. Comprueba que `metrics.stream` esté habilitado en la configuración.".into(),
        });
    }
    Ok(candidates)
}

fn process_id(endpoint: &Path) -> Option<u32> {
    let name = endpoint.file_name()?.to_str()?;
    let value = name.strip_prefix("kotavel-")?.strip_suffix(".endpoint")?;
    let pid = value.parse::<u32>().ok()?;
    (pid > 0).then_some(pid)
}

fn valid_endpoint(endpoint: &Path) -> bool {
    let Ok(contents) = fs::read_to_string(endpoint) else {
        return false;
    };
    contents
        .lines()
        .next()
        .and_then(|line| line.trim().parse::<u16>().ok())
        .is_some_and(|port| port > 0)
}

#[cfg(unix)]
fn process_belongs_to_project(process: &Process, project: &Path) -> bool {
    let Ok(project) = project.canonicalize() else {
        return false;
    };
    let Some(executable_path) = process.exe() else {
        return false;
    };
    let executable_path = if executable_path.is_absolute() {
        executable_path.to_owned()
    } else {
        project.join(executable_path)
    };
    let Some(executable) = executable_path.canonicalize().ok() else {
        return false;
    };
    let executable_roots = ["build", "target", "dist"]
        .into_iter()
        .filter_map(|directory| project.join(directory).canonicalize().ok())
        .collect::<Vec<_>>();
    if !executable_roots
        .iter()
        .any(|root| executable.starts_with(root))
    {
        return false;
    }
    let project_arguments = process
        .cmd()
        .iter()
        .skip(1)
        .filter_map(|argument| argument.to_str()?.strip_prefix("--project-root="));
    let mut has_project_argument = false;
    let project_argument_matches = project_arguments.into_iter().any(|root| {
        has_project_argument = true;
        Path::new(root)
            .canonicalize()
            .is_ok_and(|root| same_path(&root, &project))
    });
    !has_project_argument || project_argument_matches
}

#[cfg(windows)]
fn process_belongs_to_project(pid: u32, project: &Path) -> bool {
    let Some(executable) = windows_process_image(pid).and_then(|path| path.canonicalize().ok())
    else {
        return false;
    };
    let Ok(project) = project.canonicalize() else {
        return false;
    };
    let mut executable_roots = ["build", "target", "dist"]
        .into_iter()
        .filter_map(|directory| project.join(directory).canonicalize().ok())
        .collect::<Vec<_>>();
    if let Some(target_dir) = env::var_os("CARGO_TARGET_DIR")
        .and_then(|directory| PathBuf::from(directory).canonicalize().ok())
    {
        executable_roots.push(target_dir);
    }
    executable_roots
        .iter()
        .any(|root| executable.starts_with(root))
}

#[cfg(windows)]
fn windows_process_image(pid: u32) -> Option<PathBuf> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
    };

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    let mut buffer = vec![0u16; 32_768];
    let mut length = buffer.len() as u32;
    let result =
        unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) };
    unsafe { CloseHandle(process) };
    (result != 0).then(|| PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length as usize])))
}

#[cfg(any(unix, test))]
fn same_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

#[cfg(unix)]
fn process_is_running(system: &mut System, pid: u32, project: &Path) -> Result<bool, String> {
    let process_id = Pid::from(pid as usize);
    system.refresh_processes(ProcessesToUpdate::Some(&[process_id]), true);
    let Some(process) = system.process(process_id) else {
        return Ok(false);
    };
    if matches!(
        process.status(),
        ProcessStatus::Zombie | ProcessStatus::Dead
    ) {
        return Ok(false);
    }
    if !process_belongs_to_project(process, project) {
        return Err("El PID ya no corresponde a esta aplicación; no se cerró otro proceso.".into());
    }
    Ok(true)
}

#[cfg(windows)]
fn process_is_running(pid: u32, project: &Path) -> Result<bool, String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, STILL_ACTIVE},
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Ok(false);
    }
    let mut exit_code = 0;
    let queried = unsafe { GetExitCodeProcess(process, &mut exit_code) } != 0;
    unsafe { CloseHandle(process) };
    if !queried || exit_code != STILL_ACTIVE as u32 {
        return Ok(false);
    }
    if !process_belongs_to_project(pid, project) {
        return Err("El PID ya no corresponde a esta aplicación; no se cerró otro proceso.".into());
    }
    Ok(true)
}

#[cfg(unix)]
fn request_process_stop(system: &mut System, pid: u32, project: &Path) -> Result<(), String> {
    let process_id = Pid::from(pid as usize);
    system.refresh_processes(ProcessesToUpdate::Some(&[process_id]), true);
    let process = system
        .process(process_id)
        .ok_or_else(|| format!("El proceso Kotavel {pid} ya terminó."))?;
    if !process_belongs_to_project(process, project) {
        return Err("El PID ya no corresponde a esta aplicación; no se envió la señal.".into());
    }
    if process.kill_with(Signal::Term) == Some(true) {
        return Ok(());
    }
    Err("No se pudo enviar SIGTERM al proceso Kotavel.".into())
}

#[cfg(windows)]
fn request_process_stop(pid: u32, project: &Path) -> Result<(), String> {
    if !process_belongs_to_project(pid, project) {
        return Err("El PID ya no corresponde a esta aplicación; no se envió la señal.".into());
    }
    request_windows_stop(pid)
}

#[cfg(unix)]
fn force_process_stop(system: &mut System, pid: u32, project: &Path) -> Result<(), String> {
    let process_id = Pid::from(pid as usize);
    system.refresh_processes(ProcessesToUpdate::Some(&[process_id]), true);
    let process = system
        .process(process_id)
        .ok_or_else(|| format!("El proceso Kotavel {pid} ya terminó."))?;
    if !process_belongs_to_project(process, project) {
        return Err("El PID ya no corresponde a esta aplicación; no se cerró otro proceso.".into());
    }
    if process.kill() {
        Ok(())
    } else {
        Err("No se pudo forzar el cierre del proceso Kotavel.".into())
    }
}

#[cfg(windows)]
fn force_process_stop(pid: u32, project: &Path) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, TerminateProcess,
        },
    };

    if !process_belongs_to_project(pid, project) {
        return Err("El PID ya no corresponde a esta aplicación; no se cerró otro proceso.".into());
    }
    let process = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            0,
            pid,
        )
    };
    if process.is_null() {
        return Err(format!("No se pudo abrir el proceso Kotavel {pid}."));
    }
    let terminated = unsafe { TerminateProcess(process, 1) } != 0;
    unsafe { CloseHandle(process) };
    if terminated {
        Ok(())
    } else {
        Err("No se pudo forzar el cierre del proceso Kotavel.".into())
    }
}

#[cfg(unix)]
fn stop_instance(system: &mut System, instance: &Instance, project: &Path) -> Result<bool, String> {
    request_process_stop(system, instance.pid, project)?;
    let graceful_deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < graceful_deadline {
        if !process_is_running(system, instance.pid, project)? {
            remove_endpoint(&instance.endpoint)?;
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(100));
    }
    if !process_is_running(system, instance.pid, project)? {
        remove_endpoint(&instance.endpoint)?;
        return Ok(false);
    }

    force_process_stop(system, instance.pid, project)?;
    let force_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < force_deadline {
        if !process_is_running(system, instance.pid, project)? {
            remove_endpoint(&instance.endpoint)?;
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "No se pudo detener el proceso Kotavel {}.",
        instance.pid
    ))
}

#[cfg(windows)]
fn stop_instance(instance: &Instance, project: &Path) -> Result<bool, String> {
    let mut forced = false;
    if request_process_stop(instance.pid, project).is_err() {
        force_process_stop(instance.pid, project)?;
        forced = true;
    } else {
        let graceful_deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < graceful_deadline {
            if !process_is_running(instance.pid, project)? {
                remove_endpoint(&instance.endpoint)?;
                return Ok(false);
            }
            thread::sleep(Duration::from_millis(100));
        }
        if process_is_running(instance.pid, project)? {
            force_process_stop(instance.pid, project)?;
            forced = true;
        }
    }
    let force_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < force_deadline {
        if !process_is_running(instance.pid, project)? {
            remove_endpoint(&instance.endpoint)?;
            return Ok(forced);
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "No se pudo detener el proceso Kotavel {}.",
        instance.pid
    ))
}

fn remove_endpoint(endpoint: &Path) -> Result<(), String> {
    match fs::remove_file(endpoint) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "No se pudo retirar el endpoint de métricas: {error}"
        )),
    }
}

#[cfg(windows)]
fn request_windows_stop(pid: u32) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::{
        System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent},
        UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, PostMessageW, WM_CLOSE},
    };
    use windows_sys::core::BOOL;

    struct WindowSearch {
        pid: u32,
        posted: bool,
    }
    unsafe extern "system" fn close_matching_window(window: HWND, data: LPARAM) -> BOOL {
        let search = unsafe { &mut *(data as *mut WindowSearch) };
        let mut owner = 0;
        unsafe { GetWindowThreadProcessId(window, &mut owner) };
        if owner == search.pid && unsafe { PostMessageW(window, WM_CLOSE, 0, 0) } != 0 {
            search.posted = true;
        }
        1
    }

    let sent = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) } != 0;
    if sent {
        return Ok(());
    }
    let mut search = WindowSearch { pid, posted: false };
    unsafe {
        EnumWindows(
            Some(close_matching_window),
            &mut search as *mut WindowSearch as LPARAM,
        );
    }
    if search.posted {
        Ok(())
    } else {
        Err("No se pudo enviar el cierre ordenado a la aplicación Kotavel.".into())
    }
}

#[cfg(test)]
#[path = "../../tests/unit/bin/kotavel-stop.rs"]
mod tests;
