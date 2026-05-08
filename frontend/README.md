# Kubinate frontend

SvelteKit 2 + Svelte 5 on the Cloudflare Pages adapter.

## Dev

```
npm install
npm run dev
```

The dev server proxies `/api/*` to the Rust backend at
`http://localhost:8080`. Start the backend via `docker compose up -d`
and `cargo run -p kubinate-api` from the repo root.

## Build

```
npm run build
```

Produces the Cloudflare Pages bundle in `.svelte-kit/cloudflare/`.
