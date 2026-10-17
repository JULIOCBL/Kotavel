# Convención de nombres

Kotavel distingue los componentes por su propietario, su capacidad y su función.
Los nombres no deben depender de la biblioteca que implemente una capacidad.

## Prefijos

- **Framework:** el namespace `kotavel::` identifica sus tipos públicos. Por
  ejemplo, `kotavel::ServiceProvider` y `kotavel::HttpKernel`.
- **Aplicación consumidora:** cada clase propia comienza con un prefijo estable
  del proyecto o producto. Por ejemplo, `VentasAppServiceProvider`.
- **Bibliotecas externas:** sus nombres se reservan para implementaciones
  internas de adaptadores; no aparecen en los providers ni contratos de la app.

En C++, el namespace ya proporciona el prefijo del framework. No se repite en
la clase: `kotavel::HttpKernel` es preferible a `KotavelHttpKernel` dentro de
`namespace kotavel`.

## Forma de los nombres de la aplicación

Usa esta composición:

```text
<Proyecto><Instancia o área><Capacidad><Rol>
```

El segmento de instancia o área se omite cuando el componente es único:

| Propósito | Ejemplo |
| --- | --- |
| Servicios compartidos de la aplicación | `VentasAppServiceProvider` |
| API pública | `VentasPublicApiServiceProvider` |
| API administrativa | `VentasAdminApiServiceProvider` |
| Portal web | `VentasPortalWebServiceProvider` |
| Backoffice web | `VentasBackofficeWebServiceProvider` |
| Escritorio de punto de venta | `VentasPosDesktopServiceProvider` |

Los nombres de clases usan PascalCase y los archivos usan `snake_case`, por
ejemplo `ventas_public_api_service_provider.hpp`. Mantén los archivos bajo la
carpeta del rol, como `app/providers/`.

## Kernels y aplicaciones

Los kernels son motores genéricos de Kotavel: `kotavel::HttpKernel` coordina
las capacidades HTTP y `kotavel::DesktopKernel` inicia la capacidad de
escritorio. API y web pueden convivir en el kernel HTTP, aunque conservan
providers, rutas y configuración con nombres distintos. Una aplicación con
varios ejecutables identifica además cada perfil, por ejemplo `ventas-public-api`
y `ventas-pos`.

El prefijo diferencia componentes y evita colisiones; no implica que compartan
instancias de memoria. Cada proceso arranca y construye sus propios servicios.
