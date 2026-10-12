@echo off
rem Starts diskdash. Runs start.ps1 with the execution policy bypassed for this
rem one script only; no system setting is changed.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0start.ps1" %*
