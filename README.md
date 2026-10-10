# Kotavel

Creado por **Julio Cesar Borges Lopez**. Distribuido bajo la [licencia MIT](LICENSE).

Base de framework C++23 cuya arquitectura y métodos están por definir.
Actualmente expone `Kotavel::Core` como target CMake `INTERFACE`, sin biblioteca
compilada, servidor ni dependencias externas.

El [blueprint propuesto de Kotavel](docs/blueprint/README.md) define la estructura,
las etapas de implementación y los criterios de rendimiento del framework.

```text
include/kotavel/   Carpeta reservada para futuras cabeceras
src/              Carpeta reservada para futura implementación
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

Las carpetas reservadas solo contienen `.gitkeep`. Añadir archivos `.cpp` en
`src/` no los compila automáticamente: el target se ajustará cuando se defina
la implementación.

## Utilizar desde el Laboratorio

Coloca Laboratory-Kotavel junto a este repositorio. Su `kotavel.json` debe
seleccionar `mode: local` y `source_dir: ../Kotavel`. Desde el Laboratorio:

```sh
python3 kotavel.py install
python3 kotavel.py run
```

La preparación necesita Python 3.9 o posterior con pip y venv y los requisitos
del compilador de la plataforma. El ejecutable actual termina inmediatamente.
En macOS y Linux debes tener un compilador compatible con C++23 en el PATH.
En Windows se necesita Python x64 y PowerShell; el instalador prepara LLVM-MinGW.
La preparación reutiliza LLVM-MinGW instalado y los binarios nativos de CMake.
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

No existe una publicación remota configurada. Las futuras dependencias internas
se declararán en las constantes literales `FRAMEWORK_REQUIRES` y
`FRAMEWORK_OPTIONS` de esta receta; actualmente ambas están vacías.
## Herramientas compartidas

`tools/kotavel_cli/` mantiene los comandos `prepare`, `install`, `build`, `test`,
`run` y `shell`, los preparadores por sistema y los perfiles predeterminados.
Cada consumidor conserva su entrada `kotavel.py`, configuración, perfiles,
receta y lockfile. Las herramientas reciben explícitamente la raíz del proyecto;
`executable` en `kotavel.json` identifica el programa que deben ejecutar.

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

Para ejecutar las pruebas de estas herramientas desde Kotavel:

```sh
python3 -m unittest discover -s tests -p "test_*.py"
```

