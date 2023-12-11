
# RustServerController Documentation

## Current Project Status: 
### Main Branch:
- Windows Build Status: [![Build Status for Windows](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_win.yml/badge.svg?branch=main)](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_win.yml)
- Linux Build Status: [![Build Status for Linux](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_linux.yml/badge.svg)](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_linux.yml)
- MacOS Build Status: [![Build Status for MacOS](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_MacOS.yml/badge.svg)](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_MacOS.yml)

### Testing Branch:
- Windows Testing Build Status: [![Build Status for Windows Testing](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_win_testing.yml/badge.svg)](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_win_testing.yml)
- Linux Testing Build Status: [![Build Status for Linux Testing](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_linux_testing.yml/badge.svg)](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_linux_testing.yml)
- MacOS Testing Build Status: [![Build Status for MacOS Testing](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_MacOS_testing.yml/badge.svg)](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_MacOS_testing.yml)

RustServerController is designed for seamless server management on remote machines, with minimal resource usage, ensuring it remains unobtrusive in most scenarios.

## Building the Project:
You can start building in two ways:

### Building from Source:
- Clone the main branch of the repository.
- Navigate to the root directory, which mirrors the structure of the GitHub repository's main branch.
- Open your terminal and execute the following command:
  ```bash
  cargo build --release
  ```
- Once built, the compiled executable can be found at `/target/release/server_host.exe`. This file is standalone and can be moved as needed.
- Note: Ensure that you have installed Cargo from [rustup.rs](http://rustup.rs). Git might also be required for cloning the repository.

### Using Pre-Compiled Executables:
- Pre-compiled executables are regularly updated and posted in the repository's releases tab.
- These executables are less secure for those familiar with Rust, as source code may not always be provided, but they are easier to use and require less technical knowledge.

# Master-Slave Architecture
The recent addition of a new feature enables a clustered setup with master and slave configurations.

## Configuring a Slave Node:
- A server is designated as a slave by setting the 'slave' line to true in its configuration.
- Being a slave means the node will neither have slaves nor attempt to connect to configured slaves, and it will not host a web UI.

## Configuring a Master to Connect to a Slave:
- On Windows, use the `ipconfig` command to obtain the IPv4 address of the slave node's host PC.
- Edit the configuration using the following template, inserting the slave's IPv4 address and port:
  ```json
  {
    "address": "<your address here>",
    "port": "<slave's port here>"
  }
  ```

### Additional Information:
- To edit a slave's configuration, use an external text editor (Notepad++ recommended).
- While chaining master nodes is possible, it is not recommended due to potential latency issues. Support is not provided for setups with more than one layer of indirection. Assistance requests for multi-indirection setups will be the user's responsibility.
