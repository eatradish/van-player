# van-player
An Tui Music Player

## Screenshot
![image](https://user-images.githubusercontent.com/19554922/171415550-636b5ca8-2374-4fe4-bcf7-03e7fbba470a.png)

## Dep

- ncurses
- libmpv
- Glibc
- C Compile
- Rust and Cargo

## Usage

Build van-player:

```
$ cargo build --release
```

Use:

```
$ ./target/release/van [URL1] [URL2]
```

- Press [=] to add volume
- Press [-] ro reduce volume
- Press [p] to pause/unpause song
- Press [l] to control playlist
- Press [Left] or [Right] to Prev/Next song
