default:
  just --list

run:
  cargo run -- --fullscreen true

web:
  cargo geng run --platform web

publish:
  cargo geng build --release --platform web
  butler push target/geng kuviman/baby-race:html5
