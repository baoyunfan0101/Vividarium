@echo off
setlocal

cd /d "%~dp0\.."

echo Installing Python dependencies...
py -m pip install -r requirements.txt
if errorlevel 1 exit /b 1

echo Installing PyInstaller...
py -m pip install pyinstaller
if errorlevel 1 exit /b 1

echo Installing frontend dependencies...
cd frontend
call npm install
if errorlevel 1 exit /b 1

echo Building frontend...
call npm run build
if errorlevel 1 exit /b 1
cd ..

echo Building Windows executable...
py -m PyInstaller --clean --noconfirm packaging\PhytoIndex.windows.spec
if errorlevel 1 exit /b 1

echo.
echo Built: dist\PhytoIndex.exe
