@echo off
set "PATH=C:\Users\poomr\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin;C:\Users\poomr\.cargo\bin;%PATH%"
cd /d "%~dp0"
echo Starting AI Provider Connector Server...
cargo run -p ai-provider-connector-server
