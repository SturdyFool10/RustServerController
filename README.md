# RustServerController

## Project Status: [![Build Win](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_win.yml/badge.svg?branch=main)](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_win.yml) [![Build Linux](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_linux.yml/badge.svg)](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_linux.yml)
This server software is for the easy control of servers deployed on another machine, using as little by way of resources as it can, you should not notice it in all but the most resource limited of scenarios
## How to build:
To get started, you can go two routes:
### Build and compile from scratch:
in order to allow you to compile this project, clone the main branch, then go into the root directory(it should look the same as the github repo from the main branch in there), from there, you want to get your terminal and run the command:
```bash
cargo build --release
```
once the build is complete, you should be able to find your compiled executable as /target/release/server_host.exe, its standalone so move it where you please!
Please note that you will need to go to <a href="http://rustup.rs">rustup</a> in order to get cargo installed and you may want git as well for easy cloning of the repo.
### go to the releases tab for official stable and complete builds
I will be updating and posting pre-compiled executables to the repo, these are less secure for someone who does know rust, since I will likely not be distributing source code with the executable versions, but will require much less technical expertise and resources
