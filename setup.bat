@echo off
echo ============================================
echo   drumgen - Setup
echo ============================================
echo.

:: Check Python is installed
python --version >nul 2>&1
if errorlevel 1 (
    echo ERROR: Python is not installed or not in PATH.
    echo.
    echo   1. Download Python from https://www.python.org/downloads/
    echo   2. IMPORTANT: Check "Add Python to PATH" during install
    echo   3. Restart this script after installing
    echo.
    pause
    exit /b 1
)

echo Found Python:
python --version
echo.

:: Create virtual environment
if not exist ".venv" (
    echo Creating virtual environment...
    python -m venv .venv
    if errorlevel 1 (
        echo ERROR: Failed to create virtual environment.
        pause
        exit /b 1
    )
    echo Virtual environment created.
) else (
    echo Virtual environment already exists.
)
echo.

:: Install dependencies
echo Installing dependencies...
call .venv\Scripts\activate.bat
pip install -r requirements.txt
if errorlevel 1 (
    echo ERROR: Failed to install dependencies.
    pause
    exit /b 1
)
echo.

:: Create output directories
if not exist "output" mkdir output
if not exist "user_cells" mkdir user_cells
echo Created output directories.
echo.

echo ============================================
echo   Setup complete!
echo   Double-click drumgen.bat to launch the GUI.
echo ============================================
pause
