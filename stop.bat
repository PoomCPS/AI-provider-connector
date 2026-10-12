@echo off
echo Stopping AI Provider Connector Server...

taskkill /F /IM ai-provider-connector-server.exe >nul 2>&1

for /f "tokens=5" %%a in ('netstat -aon ^| findstr :6173 ^| findstr LISTENING') do (
    taskkill /F /PID %%a >nul 2>&1
)

echo AI Provider Connector Server stopped.
