# PERMA RBXL Parser Service

An isolated HTTP service that parses binary Roblox place files (`.rbxl`) using the Rust `rbx_binary` crate and returns a binary `.rbxm` containing the place's top-level children. It does not replace or modify the existing PERMA key Worker.

## Endpoints

- `GET /` or `GET /health`: health check; returns `PERMA RBXL Parser online`.
- `POST /convert`: raw binary `.rbxl` bytes in the request body; returns raw `.rbxm` bytes.

Request header: `Content-Type: application/octet-stream`.

Current upload cap: 100 MiB. Real limits also depend on hosting memory/CPU, network and executor/device memory. New Roblox property encodings may need a newer parser crate.

## Deploy separately

Deploy this directory as a Docker service on a Rust/Docker host (for example Railway). Do not replace the existing Cloudflare key Worker. The host should set `PORT`; the app defaults to 8080. Test `GET /health` after deployment.

## Importer integration

After deployment, the importer needs to `POST` the local `.rbxl` bytes to `https://YOUR-PARSER-HOST/convert`, then pass the returned bytes to its existing `loadrbxm` or `readrbxm` function while keeping its place/service insertion logic enabled. This source bundle is not yet deployed and does not have a public URL. Do not put the parser URL into the existing key Worker URL.

## Important limitation

The parser can decode `.rbxl` and re-encode the top-level instances as `.rbxm`, but the executor still must expose a compatible `loadrbxm`/`readrbxm` function. The service cannot force Delta/Studio Lite to support a loader API that it does not provide.
