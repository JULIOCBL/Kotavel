#![cfg_attr(not(target_os = "macos"), allow(dead_code, unused_imports))]

use std::{
    ffi::OsString,
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

#[cfg(target_os = "macos")]
use crate::env::{Env, FileEnvironment};

#[derive(Debug)]
struct PackageOptions {
    format: PackageFormat,
    env_file: PathBuf,
    output: PathBuf,
    release: bool,
    title: Option<String>,
    version: Option<String>,
    binary: Option<String>,
    features: Option<String>,
    deploy_qt: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PackageFormat {
    App,
    Dmg,
    Pkg,
    All,
}

/// Builds a Rust binary and packages it as a macOS application bundle.
///
/// This command is intentionally implemented separately from the legacy C++
/// package scripts. It does not change or invoke the CMake build.
pub fn package_project(
    project: &Path,
    manifest: &Path,
    arguments: &[OsString],
) -> Result<i32, String> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (project, manifest, arguments);
        Err("el empaquetado de aplicaciones macOS solo está disponible en macOS".into())
    }

    #[cfg(target_os = "macos")]
    {
        package_macos(project, manifest, arguments)
    }
}

#[cfg(target_os = "macos")]
fn package_macos(project: &Path, manifest: &Path, arguments: &[OsString]) -> Result<i32, String> {
    let project = project
        .canonicalize()
        .map_err(|error| format!("no se pudo abrir el proyecto: {error}"))?;
    let manifest = manifest
        .canonicalize()
        .map_err(|error| format!("no se pudo abrir Cargo.toml: {error}"))?;
    let options = parse_package_options(arguments)?;
    let env_file = absolute_from(&project, &options.env_file);
    let env_file = env_file
        .canonicalize()
        .map_err(|error| format!("no se encontró el archivo de entorno: {error}"))?;
    if env_file.parent() != Some(project.as_path()) {
        return Err("--env-file debe estar en la raíz del proyecto".into());
    }
    let environment = environment_name(&env_file)?;
    let environment_values = FileEnvironment::load(&project, environment)
        .map_err(|error| format!("no se pudo leer APP_SCHEME del entorno: {error}"))?;
    let scheme = Env::new(&environment_values)
        .string("APP_SCHEME", "kotavel")
        .map_err(|error| format!("APP_SCHEME no es válido: {error}"))?;
    if !valid_url_scheme(&scheme) {
        return Err("APP_SCHEME debe ser un esquema URI válido de hasta 32 caracteres".into());
    }

    let metadata = cargo_metadata(&manifest)?;
    let package = matching_package(&metadata, &manifest)?;
    let binary = select_binary(package, options.binary.as_deref())?;
    let target_dir = metadata["target_directory"]
        .as_str()
        .ok_or_else(|| "Cargo metadata no contiene target_directory".to_owned())?;
    let profile = if options.release { "release" } else { "debug" };
    let source = PathBuf::from(target_dir).join(profile).join(&binary);
    let title = options.title.unwrap_or_else(|| binary.clone());
    if title.trim().is_empty() {
        return Err("el título no puede estar vacío".into());
    }
    let version = options
        .version
        .or_else(|| package["version"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "1.0.0".into());
    validate_version(&version)?;

    let output = normalized_absolute(&project, &options.output);
    validate_output(&output, &project, &source)?;
    let stage_guard = create_staging_directory(&output)?;
    let stage = stage_guard.path().to_path_buf();
    let app_name = format!("{}.app", safe_component(&title));
    let app = stage.join(&app_name);

    let mut build = Command::new("cargo");
    build.arg("build").arg("--manifest-path").arg(&manifest);
    if options.release {
        build.arg("--release");
    }
    build.arg("--bin").arg(&binary);
    if let Some(features) = &options.features {
        build.arg("--features").arg(features);
    }
    run_command(build, "no se pudo compilar la aplicación Rust")?;
    if !source.is_file() {
        return Err(format!(
            "Cargo no generó el ejecutable esperado: {}",
            source.display()
        ));
    }

    let contents = app.join("Contents");
    let resources = contents.join("Resources");
    let macos = contents.join("MacOS");
    fs::create_dir_all(&resources).map_err(|error| error.to_string())?;
    fs::create_dir_all(&macos).map_err(|error| error.to_string())?;
    fs::copy(&source, macos.join(&binary)).map_err(|error| error.to_string())?;
    fs::write(
        contents.join("Info.plist"),
        info_plist(&title, &binary, &version, &scheme),
    )
    .map_err(|error| error.to_string())?;

    let views = project_views_root(&project)?;
    if let Some(views) = &views {
        copy_tree(views, &resources.join("resources/views"))?;
    }
    let branding = project_branding_root(&project)?;
    if let Some(branding) = &branding {
        copy_tree(branding, &resources.join("assets/branding"))?;
    }
    fs::copy(
        &env_file,
        resources.join(env_file.file_name().unwrap_or_default()),
    )
    .map_err(|error| error.to_string())?;
    fs::write(
        resources.join("kotavel-environment"),
        format!("{environment}\n"),
    )
    .map_err(|error| error.to_string())?;

    let deploy_qt = options.deploy_qt
        || options.features.as_deref().is_some_and(|features| {
            features
                .split(',')
                .any(|feature| feature == "desktop-qt" || feature.ends_with("/desktop-qt"))
        });
    if deploy_qt {
        let deploy_tool = find_program("macdeployqt").ok_or_else(|| {
            "no se encontró macdeployqt; instala Qt 6 o desactiva --deploy-qt".to_owned()
        })?;
        let mut deploy = Command::new(deploy_tool);
        deploy.arg(&app);
        if let Some(views) = &views {
            deploy.arg(format!("-qmldir={}", views.display()));
        }
        deploy.arg("-always-overwrite");
        run_command(deploy, "macdeployqt no pudo preparar el bundle")?;
    }
    let mut sign = Command::new("codesign");
    sign.args(["--force", "--deep", "--sign", "-"]).arg(&app);
    run_command(sign, "codesign no pudo firmar el bundle")?;
    let mut verify = Command::new("codesign");
    verify.args(["--verify", "--deep", "--strict"]).arg(&app);
    run_command(verify, "codesign rechazó el bundle")?;

    let safe_title = safe_component(&title);
    if matches!(options.format, PackageFormat::Dmg | PackageFormat::All) {
        create_dmg(&stage, &app, &safe_title, &title, &version)?;
    }
    if matches!(options.format, PackageFormat::Pkg | PackageFormat::All) {
        create_pkg(&stage, &app, &safe_title, &version)?;
    }

    publish_staging_directory(stage_guard, &output)?;
    println!("Generado: {}", output.join(&app_name).display());
    if matches!(options.format, PackageFormat::Dmg | PackageFormat::All) {
        println!(
            "Generado: {}",
            output.join(format!("{safe_title}-{version}.dmg")).display()
        );
    }
    if matches!(options.format, PackageFormat::Pkg | PackageFormat::All) {
        println!(
            "Generado: {}",
            output.join(format!("{safe_title}-{version}.pkg")).display()
        );
    }
    Ok(0)
}

fn parse_package_options(arguments: &[OsString]) -> Result<PackageOptions, String> {
    let mut options = PackageOptions {
        format: PackageFormat::All,
        env_file: PathBuf::from(".env"),
        output: PathBuf::from("dist"),
        release: true,
        title: None,
        version: None,
        binary: None,
        features: None,
        deploy_qt: false,
    };
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let argument = argument
            .to_str()
            .ok_or_else(|| "las opciones de package deben ser UTF-8".to_owned())?;
        let (name, inline) = argument
            .split_once('=')
            .map_or((argument, None), |(name, value)| (name, Some(value)));
        let value =
            |arguments: &mut std::slice::Iter<'_, OsString>, inline: Option<&str>, name: &str| {
                inline
                    .map(str::to_owned)
                    .or_else(|| {
                        arguments
                            .next()
                            .and_then(|value| value.to_str().map(str::to_owned))
                    })
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| format!("{name} necesita un valor UTF-8"))
            };
        match name {
            "--format" => {
                options.format = match value(&mut arguments, inline, name)?.as_str() {
                    "app" => PackageFormat::App,
                    "dmg" => PackageFormat::Dmg,
                    "pkg" => PackageFormat::Pkg,
                    "all" => PackageFormat::All,
                    _ => return Err("--format acepta app, dmg, pkg o all".into()),
                }
            }
            "--env-file" => options.env_file = PathBuf::from(value(&mut arguments, inline, name)?),
            "--output" => options.output = PathBuf::from(value(&mut arguments, inline, name)?),
            "--build-type" => {
                options.release = match value(&mut arguments, inline, name)?.as_str() {
                    "Release" => true,
                    "Debug" => false,
                    _ => return Err("--build-type acepta Debug o Release".into()),
                }
            }
            "--title" => options.title = Some(value(&mut arguments, inline, name)?),
            "--version" => options.version = Some(value(&mut arguments, inline, name)?),
            "--bin" => options.binary = Some(value(&mut arguments, inline, name)?),
            "--features" => options.features = Some(value(&mut arguments, inline, name)?),
            "--deploy-qt" => {
                if inline.is_some() {
                    return Err("--deploy-qt no recibe valor".into());
                }
                options.deploy_qt = true;
            }
            _ => return Err(format!("opción package desconocida: {argument}")),
        }
    }
    Ok(options)
}

fn environment_name(path: &Path) -> Result<&'static str, String> {
    match path.file_name().and_then(|name| name.to_str()) {
        Some(".env") => Ok("production"),
        Some(".env.local") => Ok("local"),
        Some(".env.development") => Ok("development"),
        _ => Err("usa .env, .env.local o .env.development para --env-file".into()),
    }
}

fn validate_version(version: &str) -> Result<(), String> {
    let mut bytes = version.bytes();
    if !bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(&byte))
    {
        return Err("la versión debe comenzar con una letra o número y usar solo . _ + -".into());
    }
    Ok(())
}

fn safe_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches(['-', '.'])
        .to_owned();
    if sanitized.is_empty() {
        "Kotavel".into()
    } else {
        sanitized
    }
}

#[cfg(target_os = "macos")]
fn cargo_metadata(manifest: &Path) -> Result<serde_json::Value, String> {
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--no-deps",
            "--format-version=1",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()
        .map_err(|error| format!("no se pudo ejecutar cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("cargo metadata produjo JSON inválido: {error}"))
}

#[cfg(target_os = "macos")]
fn matching_package<'a>(
    metadata: &'a serde_json::Value,
    manifest: &Path,
) -> Result<&'a serde_json::Value, String> {
    metadata["packages"]
        .as_array()
        .and_then(|packages| {
            packages.iter().find(|package| {
                package["manifest_path"].as_str() == Some(manifest.to_string_lossy().as_ref())
            })
        })
        .ok_or_else(|| format!("Cargo workspace no contiene {}", manifest.display()))
}

#[cfg(target_os = "macos")]
fn select_binary(package: &serde_json::Value, selected: Option<&str>) -> Result<String, String> {
    let binaries = package["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|target| {
            target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
        })
        .filter_map(|target| target["name"].as_str())
        .collect::<Vec<_>>();
    if let Some(selected) = selected {
        return binaries
            .into_iter()
            .find(|binary| *binary == selected)
            .map(str::to_owned)
            .ok_or_else(|| format!("el binario {selected} no existe en el paquete Cargo"));
    }
    match binaries.as_slice() {
        [binary] => Ok((*binary).into()),
        [] => Err("el paquete Cargo no define un target binario".into()),
        _ => Err("el paquete tiene varios binarios; selecciona uno con --bin".into()),
    }
}

fn absolute_from(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    }
}

fn normalized_absolute(root: &Path, path: &Path) -> PathBuf {
    let path = absolute_from(root, path);
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn validate_output(output: &Path, project: &Path, source: &Path) -> Result<(), String> {
    if fs::symlink_metadata(output).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(format!(
            "el directorio de salida no puede ser un enlace simbólico: {}",
            output.display()
        ));
    }
    if output == Path::new("/")
        || output == project
        || project.starts_with(output)
        || source.starts_with(output)
        || output.starts_with(source)
    {
        return Err(format!(
            "el directorio de salida no se puede reemplazar de forma segura: {}",
            output.display()
        ));
    }
    if output.starts_with(project)
        && [
            ".git",
            "src",
            "include",
            "resources",
            "tests",
            "tools",
            "docs",
            "build",
        ]
        .iter()
        .any(|directory| output.starts_with(project.join(directory)))
    {
        return Err(format!(
            "la salida no puede reemplazar archivos del proyecto: {}",
            output.display()
        ));
    }
    if output.exists() && !output.is_dir() {
        return Err(format!(
            "la salida existe y no es un directorio: {}",
            output.display()
        ));
    }
    if output.exists()
        && fs::read_dir(output)
            .map_err(|error| error.to_string())?
            .next()
            .is_some()
    {
        let marker = output.join(".kotavel-package-state");
        let managed = fs::symlink_metadata(&marker)
            .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            && fs::read_to_string(marker)
                .is_ok_and(|contents| contents == "kotavel-rust-package-v1\n");
        if !managed {
            return Err(
                "el directorio de salida contiene archivos ajenos; usa otro directorio".into(),
            );
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn create_staging_directory(output: &Path) -> Result<tempfile::TempDir, String> {
    let parent = output
        .parent()
        .ok_or_else(|| "la ruta de salida no tiene directorio padre".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    tempfile::Builder::new()
        .prefix(".kotavel-package-")
        .tempdir_in(parent)
        .map_err(|error| format!("no se pudo crear staging del paquete: {error}"))
}

#[cfg(target_os = "macos")]
fn publish_staging_directory(stage: tempfile::TempDir, output: &Path) -> Result<(), String> {
    fs::write(
        stage.path().join(".kotavel-package-state"),
        "kotavel-rust-package-v1\n",
    )
    .map_err(|error| error.to_string())?;
    let backup = output.with_extension(format!("kotavel-backup-{}", std::process::id()));
    if backup.exists() {
        return Err(format!(
            "ya existe un respaldo de salida: {}",
            backup.display()
        ));
    }
    if output.exists() {
        fs::rename(output, &backup).map_err(|error| error.to_string())?;
    }
    if let Err(error) = fs::rename(stage.path(), output) {
        if backup.exists() {
            let _ = fs::rename(&backup, output);
        }
        return Err(format!("no se pudo publicar el paquete: {error}"));
    }
    if backup.exists() {
        fs::remove_dir_all(backup).map_err(|error| {
            format!("paquete publicado; no se pudo retirar el respaldo anterior: {error}")
        })?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_symlink() {
            return Err(format!(
                "no se empaquetan enlaces simbólicos de recursos: {}",
                source_path.display()
            ));
        } else if kind.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else if kind.is_file() {
            fs::copy(source_path, destination_path).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn project_views_root(project: &Path) -> Result<Option<PathBuf>, String> {
    let views = project.join("resources/views");
    let metadata = match fs::symlink_metadata(&views) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "la raíz de vistas debe ser un directorio real dentro del proyecto: {}",
            views.display()
        ));
    }
    let canonical = views.canonicalize().map_err(|error| error.to_string())?;
    if !canonical.starts_with(project) {
        return Err("la raíz de vistas sale del proyecto".into());
    }
    Ok(Some(canonical))
}

#[cfg(target_os = "macos")]
fn project_branding_root(project: &Path) -> Result<Option<PathBuf>, String> {
    let branding = project.join("assets/branding");
    let metadata = match fs::symlink_metadata(&branding) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "la raíz de marca debe ser un directorio real dentro del proyecto: {}",
            branding.display()
        ));
    }
    let canonical = branding.canonicalize().map_err(|error| error.to_string())?;
    if !canonical.starts_with(project) {
        return Err("la raíz de marca sale del proyecto".into());
    }
    Ok(Some(canonical))
}

#[cfg(target_os = "macos")]
fn copy_bundle_preserving_symlinks(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::unix::fs::symlink;
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_symlink() {
            let target = fs::read_link(&source_path).map_err(|error| error.to_string())?;
            symlink(target, destination_path).map_err(|error| error.to_string())?;
        } else if kind.is_dir() {
            copy_bundle_preserving_symlinks(&source_path, &destination_path)?;
        } else if kind.is_file() {
            fs::copy(source_path, destination_path).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn info_plist(title: &str, executable: &str, version: &str, scheme: &str) -> String {
    let identifier = format!("com.kotavel.{}", safe_component(executable).to_lowercase());
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>CFBundleName</key><string>{}</string>\n<key>CFBundleDisplayName</key><string>{}</string>\n<key>CFBundleExecutable</key><string>{}</string>\n<key>CFBundleIdentifier</key><string>{}</string>\n<key>CFBundlePackageType</key><string>APPL</string>\n<key>CFBundleShortVersionString</key><string>{}</string>\n<key>CFBundleVersion</key><string>{}</string>\n<key>CFBundleURLTypes</key><array><dict><key>CFBundleURLName</key><string>{}</string><key>CFBundleURLSchemes</key><array><string>{}</string></array></dict></array>\n</dict></plist>\n",
        xml_escape(title),
        xml_escape(title),
        xml_escape(executable),
        xml_escape(&identifier),
        xml_escape(version),
        xml_escape(version),
        xml_escape(&identifier),
        xml_escape(scheme)
    )
}

#[cfg(target_os = "macos")]
fn valid_url_scheme(scheme: &str) -> bool {
    let mut bytes = scheme.bytes();
    scheme.len() <= 32
        && bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'.' | b'-'))
}

fn xml_escape(value: &str) -> String {
    value.chars().fold(String::new(), |mut escaped, character| {
        escaped.push_str(match character {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            '"' => "&quot;",
            '\'' => "&apos;",
            _ => {
                escaped.push(character);
                return escaped;
            }
        });
        escaped
    })
}

#[cfg(target_os = "macos")]
fn create_dmg(
    stage: &Path,
    app: &Path,
    safe_title: &str,
    title: &str,
    version: &str,
) -> Result<(), String> {
    use std::os::unix::fs::symlink;
    let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
    let image_stage = temporary.path();
    copy_bundle_preserving_symlinks(app, &image_stage.join(app.file_name().unwrap_or_default()))?;
    symlink("/Applications", image_stage.join("Applications"))
        .map_err(|error| error.to_string())?;
    let image = stage.join(format!("{safe_title}-{version}.dmg"));
    let mut command = Command::new("hdiutil");
    command
        .args(["create", "-volname", title, "-srcfolder"])
        .arg(image_stage)
        .args(["-ov", "-format", "UDZO"])
        .arg(image);
    run_command(command, "hdiutil no pudo crear la imagen DMG")
}

#[cfg(target_os = "macos")]
fn create_pkg(stage: &Path, app: &Path, safe_title: &str, version: &str) -> Result<(), String> {
    let package = stage.join(format!("{safe_title}-{version}.pkg"));
    let mut command = Command::new("productbuild");
    command
        .args(["--component"])
        .arg(app)
        .arg("/Applications")
        .arg(package);
    run_command(command, "productbuild no pudo crear el instalador PKG")
}

#[cfg(target_os = "macos")]
fn find_program(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(target_os = "macos")]
fn run_command(mut command: Command, context: &str) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|error| format!("{context}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{context} (código {})", status.code().unwrap_or(1)))
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn environment_files_map_to_runtime_environment_names() {
        assert_eq!(environment_name(Path::new(".env")).unwrap(), "production");
        assert_eq!(environment_name(Path::new(".env.local")).unwrap(), "local");
        assert_eq!(
            environment_name(Path::new(".env.development")).unwrap(),
            "development"
        );
        assert!(environment_name(Path::new(".env.test")).is_err());
    }

    #[test]
    fn package_options_validate_form_profiles_and_unknown_switches() {
        let options = parse_package_options(&[
            "--format=dmg".into(),
            "--build-type".into(),
            "Debug".into(),
            "--features".into(),
            "desktop-qt".into(),
            "--deploy-qt".into(),
        ])
        .unwrap();
        assert_eq!(options.format, PackageFormat::Dmg);
        assert!(!options.release);
        assert_eq!(options.features.as_deref(), Some("desktop-qt"));
        assert!(options.deploy_qt);
        assert!(parse_package_options(&["--format=zip".into()]).is_err());
        assert!(parse_package_options(&["--unknown".into()]).is_err());
    }

    #[test]
    fn package_names_versions_and_plist_values_are_sanitized() {
        assert_eq!(safe_component("Kotavel POS/1"), "Kotavel-POS-1");
        assert_eq!(safe_component("..."), "Kotavel");
        assert!(validate_version("1.2.3-rc+1").is_ok());
        assert!(validate_version("../unsafe").is_err());
        assert_eq!(
            xml_escape("A & <B> \"C\""),
            "A &amp; &lt;B&gt; &quot;C&quot;"
        );
    }

    #[test]
    fn package_output_cannot_replace_project_sources_or_symlinks() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("project");
        fs::create_dir(&project).unwrap();
        let source = project.join("target/release/app");
        assert!(validate_output(directory.path(), &project, &source).is_err());
        assert!(validate_output(&project, &project, &source).is_err());
        assert!(validate_output(&project.join("target"), &project, &source).is_err());
        assert!(validate_output(&project.join("resources"), &project, &source).is_err());

        let output = directory.path().join("linked");
        #[cfg(unix)]
        std::os::unix::fs::symlink(directory.path(), &output).unwrap();
        assert!(validate_output(&output, &project, &source).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn package_rejects_views_that_are_symlinked_outside_the_project() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("project");
        let outside = directory.path().join("outside");
        fs::create_dir_all(project.join("resources")).unwrap();
        fs::create_dir(&outside).unwrap();
        symlink(&outside, project.join("resources/views")).unwrap();

        assert!(project_views_root(&project).is_err());
    }

    #[test]
    fn packages_a_built_rust_binary_into_a_signed_app_bundle() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path();
        fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"sample-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::create_dir(project.join("src")).unwrap();
        fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(project.join(".env"), "APP_ENV=production\n").unwrap();

        assert_eq!(
            package_project(
                project,
                &project.join("Cargo.toml"),
                &[
                    "--format=all".into(),
                    "--env-file=.env".into(),
                    "--output=dist".into(),
                    "--build-type=Debug".into(),
                    "--title=Sample App".into(),
                    "--version=1.2.3".into(),
                ],
            )
            .unwrap(),
            0
        );

        let app = project.join("dist/Sample-App.app");
        assert!(app.join("Contents/MacOS/sample-app").is_file());
        assert!(app.join("Contents/Resources/.env").is_file());
        assert!(app.join("Contents/Info.plist").is_file());
        assert!(
            Command::new("plutil")
                .args(["-lint"])
                .arg(app.join("Contents/Info.plist"))
                .status()
                .unwrap()
                .success()
        );
        assert!(project.join("dist/.kotavel-package-state").is_file());
        assert!(project.join("dist/Sample-App-1.2.3.dmg").is_file());
        assert!(project.join("dist/Sample-App-1.2.3.pkg").is_file());
    }
}
