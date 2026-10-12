Write-Host "Stopping AI Provider Connector Server..." -ForegroundColor Yellow

# Kill by executable name
Get-Process -Name "ai-provider-connector-server" -ErrorAction SilentlyContinue | Stop-Process -Force

# Kill any process listening on port 6173
Get-NetTCPConnection -LocalPort 6173 -State Listen -ErrorAction SilentlyContinue | ForEach-Object {
    Stop-Process -Id $_.OwningProcess -Force -ErrorAction SilentlyContinue
}

Write-Host "AI Provider Connector Server stopped." -ForegroundColor Green
