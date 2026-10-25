# Escritorio

El módulo `desktop` registra destinos de navegación, valida deep links y administra el ciclo de ventanas mediante los traits `DesktopPlatform` y `WindowManager`. El adaptador Qt es opcional y se activa con la feature correspondiente.

## Ejemplo de ruta

```rust
use kotavel::desktop::{DesktopRouter, Route, WindowSettings, view};

let mut router = DesktopRouter::new();
let home = view("desktop.home")?;
let window = WindowSettings::default();
let mut routes = Route::desktop(&mut router);
routes.get("/", move |desktop| {
    let _ = desktop.show("main", &window, &home);
})?.name("desktop.home")?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Las vistas se resuelven bajo el directorio configurado. Mantén navegación desktop separada de rutas HTTP; el deep link se valida antes de despachar la vista.

## API del router y contexto

- `DesktopRouter::new/contains/dispatch`: crear, comprobar y despachar un path.
- `Route::desktop(router)`, `get(path, handler)`, `name(value)`: declarar
  destinos con un nombre opcional único.
- `DesktopKernel::new/with_options`, `with_session_context`, `routes` y
  `run(arguments)`: componer plataforma, rutas y argumentos de arranque.
- `view(name)`, `View::resource_path/resolve(root)`: validar identificadores
  `desktop.*` y resolver QML dentro del directorio permitido.
- `DesktopContext::show/close/is_visible/navigate/session_context`: controlar
  ventanas y navegación durante la vida corta del callback.
- `dispatch_deep_link(uri, scheme)`: convertir y validar un URI del esquema
  registrado antes del despacho.
- `WindowManager` y `DesktopPlatform`: contratos para crear adaptadores; el
  feature `desktop-qt` incluye `QtDesktopPlatform`.

La ruta inicial `/` es obligatoria. Un callback no debe guardar su
`DesktopContext`; solo es válido durante el despacho. Las rutas desktop no
admiten middleware HTTP ni verbs distintos de GET.
