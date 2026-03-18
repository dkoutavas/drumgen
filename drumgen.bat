@echo off
echo Starting drumgen GUI...
echo.

:: Check venv exists
if not exist ".venv\Scripts\activate.bat" (
    echo ERROR: Virtual environment not found.
    echo   Run setup.bat first.
    echo.
    pause
    exit /b 1
)

:: Activate venv and launch Streamlit
call .venv\Scripts\activate.bat
streamlit run app.py
if errorlevel 1 (
    echo.
    echo drumgen exited with an error. See above for details.
)
pause
