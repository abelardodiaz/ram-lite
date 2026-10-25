# Instala ram-lite en su ruta fija y lo arranca:
#   1. compila en release (CARGO_TARGET_DIR si esta definido; si no, %TEMP%\ram-lite-target),
#   2. cierra el ram-lite que este corriendo (cierre limpio: quita su icono de la bandeja),
#   3. copia el exe a %LOCALAPPDATA%\ram-lite\ram-lite.exe,
#   4. lo arranca desde ahi.
# "Iniciar con Windows" (menu de la bandeja) apunta a esa misma ruta fija.
# Uso: powershell -ExecutionPolicy Bypass -File instalar.ps1

$ErrorActionPreference = 'Stop'

$crate = $PSScriptRoot
if (-not $env:CARGO_TARGET_DIR) {
    $env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'ram-lite-target'
}
$name = 'ram-lite'
$built = [System.IO.Path]::GetFullPath((Join-Path $env:CARGO_TARGET_DIR "release\$name.exe"))
$installDir = Join-Path $env:LOCALAPPDATA $name
$installed = Join-Path $installDir "$name.exe"

# Cierre limpio: WM_CLOSE a la ventana (clase RamLite) para que quite el icono.
Add-Type -Namespace RamLite -Name Win -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)]
public static extern System.IntPtr FindWindowW(string cls, string title);
[DllImport("user32.dll")]
public static extern bool PostMessageW(System.IntPtr hwnd, uint msg, System.IntPtr w, System.IntPtr l);
'@

function Close-RamLite {
    $running = @(Get-Process -Name $name -ErrorAction SilentlyContinue)
    if ($running.Count -eq 0) { return }
    Write-Host "Cerrando el ram-lite que esta corriendo (PID $($running.Id -join ', '))..."
    $hwnd = [RamLite.Win]::FindWindowW('RamLite', [NullString]::Value)
    if ($hwnd -ne [System.IntPtr]::Zero) {
        [void][RamLite.Win]::PostMessageW($hwnd, 0x0010, [System.IntPtr]::Zero, [System.IntPtr]::Zero)
    }
    foreach ($p in $running) {
        if (-not $p.WaitForExit(5000)) {
            Write-Host "No cerro a tiempo; se termina el PID $($p.Id)."
            Stop-Process -Id $p.Id -Force
        }
    }
}

# Si el que corre es el propio exe de compilacion, hay que cerrarlo antes: Windows lo bloquea.
$fromBuild = @(Get-Process -Name $name -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $built })
if ($fromBuild.Count -gt 0) { Close-RamLite }

Write-Host "Compilando ($env:CARGO_TARGET_DIR)..."
Push-Location $crate
try {
    & cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw "cargo build fallo (codigo $LASTEXITCODE)" }
} finally {
    Pop-Location
}

Close-RamLite

New-Item -ItemType Directory -Force -Path $installDir | Out-Null
Copy-Item -Path $built -Destination $installed -Force
Write-Host "Instalado en $installed"

Start-Process -FilePath $installed
Start-Sleep -Milliseconds 800
$now = @(Get-Process -Name $name -ErrorAction SilentlyContinue)
if ($now.Count -eq 0) { throw 'ram-lite no quedo corriendo.' }
Write-Host "Corriendo (PID $($now.Id -join ', '))."
