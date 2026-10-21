# Kotavel

Creado por **Julio Cesar Borges Lopez**. Distribuido bajo la [licencia MIT](LICENSE).

Framework C++23 en desarrollo. `Kotavel::Core` es una biblioteca estática con
ciclo de vida síncrono, recuperación explícita de operaciones y diagnóstico
a archivo y consola, con ubicación, trazas y presentación por entorno. El target
opcional `Kotavel::Http` incorpora un adaptador Drogon; el núcleo no depende de
él. Consulta [rutas HTTP y deep links](docs/http.md).

Consulta [Configuración](docs/configuration.md) para el entorno y `Config`,
[el uso del núcleo](docs/core.md) para registros y errores,
[el contenedor de servicios](docs/container.md) para registrar dependencias,
[rutas HTTP y deep links](docs/http.md) y su [referencia de middleware](docs/http/middleware.md).
El [módulo desktop](docs/desktop.md) documenta rutas, ventanas y vistas QML.
La [referencia interna de Config](docs/internals/config.md) explica su
almacenamiento, carga, coste de operaciones y reglas de concurrencia.

El [blueprint propuesto de Kotavel](docs/blueprint/README.md) define la estructura,
las etapas de implementación y los criterios de rendimiento del framework. Los [microbenchmarks](benchmarks/README.md)
permiten reproducir las primeras mediciones del núcleo.

```text
include/kotavel/   Interfaces públicas del núcleo, HTTP opcional y deep links
src/              Implementación del núcleo y módulos
resources/messages/ Catálogos JSON de mensajes internos por módulo
assets/branding/  Logotipo, símbolo e icono predeterminados de Kotavel
CMakeLists.txt    Target público e instalación
conanfile.py      Resolución de dependencias y empaquetado
tools/kotavel_cli/ Herramientas Python compartidas
pyproject.toml    Paquete kotavel-tools
tests/            Pruebas de las herramientas
adapters/         Integraciones futuras con bibliotecas externas
templates/application/ Plantilla futura para proyectos consumidores
examples/         Ejemplos futuros de consumo
benchmarks/       Mediciones futuras de rendimiento
docs/blueprint/   Diseño propuesto y decisiones pendientes
```

## Identidad visual

Los recursos predeterminados de marca están en `assets/branding/`:

- `kotavel-logo.png`: logotipo completo con el nombre Kotavel.
- `kotavel-mark.png`: símbolo geométrico de Kotavel.
- `kotavel-icon.png`: símbolo con llave ajustable.

Las imágenes se conservaron tal como fueron proporcionadas, incluidos sus fondos.

HTTP, deep links, middleware HTTP y el ciclo inicial desktop ya tienen contratos.
El adaptador Qt es opcional; la asociación de deep links y la administración
reiniciable de servicios siguen pendientes. Cada fuente se incorpora
explícitamente a su target CMake.

Los textos internos se obtienen mediante `Messages::get()` en `src/text/messages.hpp`.
La clave combina el nombre del catálogo con su ruta anidada, por ejemplo
`env.open_failed.windows.description`; los parámetros `{{"file", ".env"}}` sustituyen
marcadores como `:file`. Los nombres usan letras ASCII, números y `_`, sin
empezar por un número. Los valores se insertan literalmente y no se reinterpretan.
Una clave válida pero desconocida lanza `std::out_of_range`; una clave mal formada,
un parámetro requerido ausente, duplicado, sobrante o con nombre inválido lanza
`std::invalid_argument`. CMake valida y convierte `resources/messages/*.json` en cadenas
compiladas: no se leen catálogos en ejecución ni se agrega un parser al núcleo.
El nombre `open_failed` incluye cualquier fallo al abrir el entorno, no solo
la ausencia del archivo. Este módulo prepara textos; la consola decide cómo mostrarlos.

La detección del sistema operativo está disponible en
`<kotavel/system/os.hpp>` mediante `kotavel::OS::type` y los helpers
`isWindows()`, `isMac()`, `isLinux()` y `name()`. Para mostrar el nombre usa
`std::cout << kotavel::OS::name()`; `type` se conserva para comparaciones.

## Utilizar desde el Laboratorio

Coloca Laboratory-Kotavel junto a este repositorio. Su `kotavel.json` debe
seleccionar `mode: local` y `source_dir: ../Kotavel`. Desde el Laboratorio:

```sh
python3 kotavel.py install
python3 kotavel.py run
```

La preparación necesita Python 3.9 o posterior con pip y venv y los requisitos
del compilador de la plataforma. El ejecutable realiza una operación y termina ordenadamente.
En macOS y Linux debes tener un compilador compatible con C++23 en el PATH.
En Windows se necesita Python x64 y PowerShell; el instalador prepara LLVM-MinGW.
La preparación reutiliza LLVM-MinGW instalado y los binarios nativos de CMake.
En Windows, Conan usa una caché compartida en `%USERPROFILE%\\.k` para que las
rutas generadas por Qt no excedan el límite de longitud de Windows.
En Windows, el entorno generado selecciona `mingw64` para OpenSSL: LLVM-MinGW
usa el ABI GNU y la receta de Conan selecciona Visual C++ por defecto para Clang.
Esta configuración se conserva en `install`, `build`, `run` y la terminal activada.
El perfil Windows compila las bibliotecas de OpenSSL sin su utilidad `openssl.exe`,
que Kotavel no utiliza y cuya receta no incorpora todos los helpers de Clang/MinGW.
Al repetir la preparación en Windows, las rutas de las herramientas locales
se colocan al principio del PATH sin acumular copias, incluso con el entorno
ya activo. Las demás entradas conservan su orden y contenido.
El README de Laboratory-Kotavel contiene los pasos por sistema.

En Linux, la preparación reutiliza el entorno sin acumular rutas en el PATH.
CMake y CTest se ejecutan directamente desde sus binarios nativos, también
cuando Python utiliza `lib64` o un enlace entre `lib` y `lib64`.
Si no existen `gcc` y `g++` sin sufijo, se busca una pareja versionada disponible
en el PATH. El perfil conserva C++23 y detecta x86_64 o ARM64.

## Empaquetar la base

Desde el Laboratorio, abre su entorno local y crea el paquete:

```sh
python3 kotavel.py shell
conan create ../Kotavel --build=missing
exit
```

Esto crea `kotavel/0.1.0` en la caché del Laboratorio. Para consumirlo, cambia
su configuración a `mode: package` y `version: 0.1.0`, y ejecuta dependencias
y compilación nuevamente. La guía `docs/framework.md` del Laboratorio explica
la selección de origen.

No existe una publicación remota configurada. Las dependencias obligatorias y
opcionales del framework se declaran en las constantes literales de esta receta.
## Herramientas compartidas

`tools/kotavel_cli/` mantiene los comandos `prepare`, `install`, `build`, `test`,
`run`, `package` y `shell`, los preparadores por sistema y los perfiles predeterminados.
Cada consumidor conserva su entrada `kotavel.py`, configuración, perfiles,
receta y lockfile. Las herramientas reciben explícitamente la raíz del proyecto.
`APP_NAME` en el archivo de entorno seleccionado identifica la aplicación;
si falta, el framework usa `Kotavel`.

En modo local, la entrada carga las herramientas desde este repositorio.
En modo paquete necesita `kotavel-tools` instalado en el Python del consumidor.
Desde un entorno virtual activo puedes instalarlo desde la raíz de Kotavel:

```sh
python3 -m pip install .
```

`tools_version` fija su versión independientemente del paquete Conan. No hay
publicación en un índice Python ni generador de proyectos nuevos todavía.
Los perfiles propios del consumidor tienen prioridad sobre los predeterminados
incluidos en las herramientas.

En macOS, `package` compila el proyecto con el archivo de entorno seleccionado y
genera los formatos solicitados. Por defecto usa `.env` y genera los tres
artefactos dentro de `dist/`:

```sh
python3 kotavel.py package
python3 kotavel.py package --env-file .env.development --format pkg
python3 kotavel.py package --format dmg --output build/distribution
```

`--env-file` admite `.env`, `.env.local` y `.env.development`; por omisión,
`.env` corresponde al entorno `production`. `--format` acepta `app`, `dmg`,
`pkg` o `all`. El `.app` lo crea CMake y el comando lo copia al directorio de
salida; `.dmg` es la imagen de arrastre y `.pkg` es el instalador de macOS. La
generación incluye las vistas del consumidor, las bibliotecas y módulos Qt
necesarios y la configuración seleccionada. Al primer arranque, Kotavel copia
el archivo de entorno a `~/Library/Application Support/<Aplicación>` para que
los registros y los datos de ejecución tengan una ubicación escribible.
Requiere las herramientas nativas de macOS `hdiutil`, `productbuild`,
`codesign` y `macdeployqt`. Cada ejecución empaqueta únicamente el `.app`
generado por el proyecto seleccionado.

Para ejecutar las pruebas de estas herramientas desde Kotavel:

```sh
python3 -m unittest discover -s tests -p "test_*.py"
```

