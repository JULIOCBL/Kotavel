# Aplicaciones de escritorio

`Kotavel::Desktop` contiene las rutas, el ciclo inicial y los contratos de
ventana. El adaptador opcional `Kotavel::DesktopQt` usa Qt Quick para crear
ventanas y cargar vistas QML. Las aplicaciones que no activan `desktop` no
resuelven Qt ni inician un loop gráfico.

## Ruta inicial y vistas

Las rutas desktop usan un router separado del HTTP. El consumidor registra `/`;
`DesktopKernel::run()` la despacha al arrancar y exige que exista:

```cpp
auto Route = kotavel::desktop::Route::desktop(kernel.routes());
Route.get("/", [](kotavel::desktop::DesktopContext& desktop) {
    desktop.show("main", {"Kotavel", 1100, 720},
                 kotavel::desktop::view("desktop.home"));
}).name("desktop.home");
```

`desktop.show()` crea o reutiliza la ventana `main`, carga la vista y la muestra.
`view()` valida identificadores bajo `desktop.*`; por ejemplo,
`desktop.pos.home` busca `resources/views/desktop/pos/home.qml`. Los assets
estáticos compartidos con una interfaz web pueden colocarse en `public/`.

Una ruta adicional se registra con `.get("/pos/home", handler)`. Una URI
`kotavel://pos/home` se traduce a `/pos/home`; al iniciar el proceso con esa URI,
el deep link reemplaza la ruta inicial. La asociación del esquema con el
ejecutable debe configurarse desde el instalador de cada plataforma y sigue
pendiente.

## Ventanas y navegación

El callback recibe un `DesktopContext` de vida limitada. Puede mostrar y cerrar
ventanas, consultar si están visibles y pedir al router que navegue a otra ruta.
No guardes ese contexto al salir del callback. `DesktopPlatform` es el contrato
que permite sustituir Qt por otro adaptador.

`QtDesktopPlatform` crea una sola `QGuiApplication` en el hilo principal, prepara
las rutas y ejecuta el loop de eventos. El host conserva las ventanas por id y
carga las vistas QML desde el directorio indicado por `DESKTOP_VIEWS`.

## HTTP y escritorio juntos

Cuando HTTP y desktop están activos, Drogon ejecuta su loop en un hilo de
servicio y Qt conserva el hilo principal. Kotavel espera a que Drogon confirme
el inicio antes de abrir la ventana, y pide el cierre HTTP al salir del loop
gráfico. Con `DESKTOP_ENABLED=false`, HTTP sigue ejecutándose como servicio
bloqueante.

El módulo permite abrir y cerrar ventanas, pero todavía no incluye un
administrador para detener y reiniciar HTTP desde controles de la interfaz. Eso
requiere un ciclo reiniciable del servidor antes de ofrecer esos controles.

## Dependencia y distribución

Conan incorpora `qt/6.11.1` solo si el consumidor activa la característica
`desktop`. El adaptador se enlaza dinámicamente. Qt se distribuye bajo LGPL/GPL
o licencia comercial; la distribución con la licencia open source requiere
cumplir sus condiciones, incluyendo avisos, fuentes correspondientes y la
posibilidad de reemplazar/re-enlazar las bibliotecas. Revisa las
[obligaciones LGPL/GPL de Qt](https://www.qt.io/development/open-source-lgpl-obligations)
antes de distribuir una aplicación.

Las pruebas del núcleo usan una plataforma falsa y no requieren una sesión
gráfica:

```sh
ctest --test-dir build/core -C Debug -R '^kotavel\\.desktop$' --output-on-failure
```
