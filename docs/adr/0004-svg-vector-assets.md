# SVG is a vector asset

SnappyRef treats SVG imports as vector assets rather than forcing them through the normal raster decode, JPEG/WebP canvas cache, and canvas image LOD pipeline. The app preserves the original SVG bytes as the asset representation and lets egui's SVG renderer handle display-size rasterization at render time.

This keeps SVGs sharp when scaled and avoids turning a vector source into a lossy or fixed-resolution cache artifact. The trade-off is that SVG needs explicit exceptions where the app expects raster pixels, such as crop, semantic-search indexing, and future preview derivatives.

## Consequences

- SVG imports should preserve original SVG bytes and MIME identity.
- SVG canvas rendering should use the vector path rather than persisted raster cache files.
- Cropping SVG is rejected until there is an explicit rasterization workflow.
- Semantic-search indexing and raster cache backfill should stay skipped for SVG until named preview derivatives exist.
