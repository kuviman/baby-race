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

publish-itch:
  CONNECT=wss://baby-race.kuviman.com cargo geng build --release --platform web
  butler push target/geng kuviman/baby-race:html5

update-server:
  docker run --rm -it -e CARGO_TARGET_DIR=/target -v `pwd`/docker-target:/target -v `pwd`:/src -w /src ghcr.io/geng-engine/cargo-geng cargo geng build --release
  rsync -avz docker-target/geng/ ees@baby-race.kuviman.com:baby-race/
  ssh ees@baby-race.kuviman.com systemctl --user restart baby-race

deploy:
  just update-server
  just publish-itch
