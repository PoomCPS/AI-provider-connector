$env:PATH = "C:\Users\poomr\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin;C:\Users\poomr\.cargo\bin;" + $env:PATH
Set-Location -Path $PSScriptRoot
Write-Host "Starting AI Provider Connector Server on http://localhost:6173/dashboard ..." -ForegroundColor Green
cargo run -p ai-provider-connector-server
