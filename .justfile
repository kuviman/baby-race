default:
  just --list

run-server:
  cargo run -- --server 127.0.0.1:1155

run:
  cargo run -- --fullscreen true

serve-web:
  CONNECT=ws://localhost:1155 cargo geng serve --platform web

web:
  cargo build
  just run-server & just serve-web && fg

publish:
  cargo geng build --release --platform web
  butler push target/geng kuviman/baby-race:html5
