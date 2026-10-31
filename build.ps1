param([switch]$SkipNative)
$ErrorActionPreference = 'Stop'
$taskRoot = $PSScriptRoot
$env:DOTNET_CLI_HOME = Join-Path $taskRoot '.dotnet'
$env:NUGET_PACKAGES = Join-Path $taskRoot '.nuget'
$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
$env:DOTNET_GENERATE_ASPNET_CERTIFICATE = 'false'
if (Test-Path (Join-Path $taskRoot '.toolchain/rustup/settings.toml')) {
    $env:CARGO_HOME = Join-Path $taskRoot '.toolchain/cargo'
    $env:RUSTUP_HOME = Join-Path $taskRoot '.toolchain/rustup'
    $env:Path = "$env:CARGO_HOME\bin;$env:Path"
}
$out = Join-Path $taskRoot 'release/TorchlightHelper-KO'
New-Item -ItemType Directory -Path $out -Force | Out-Null
if (!$SkipNative) {
    # Rust embeds source locations in panic messages. Keep local account paths out of releases.
    $oldRustFlags = $env:CARGO_ENCODED_RUSTFLAGS
    $remapFlags = @("--remap-path-prefix=$taskRoot=/_/torchlight-overlay", "--remap-path-prefix=$($taskRoot.Replace('\', '/'))=/_/torchlight-overlay") -join [char]31
    $env:CARGO_ENCODED_RUSTFLAGS = if ($oldRustFlags) { $oldRustFlags + [char]31 + $remapFlags } else { $remapFlags }
    try {
        & cargo build --release --locked --manifest-path (Join-Path $taskRoot 'vorax/Cargo.toml')
        if ($LASTEXITCODE -ne 0) { throw 'Native build failed.' }
    } finally {
        $env:CARGO_ENCODED_RUSTFLAGS = $oldRustFlags
    }
}
& dotnet publish (Join-Path $taskRoot 'tartarus/src/Tartarus.App/Tartarus.App.csproj') -c Release -p:Platform=x64 -r win-x64 --self-contained true -p:SentryDsn= -o $out
if ($LASTEXITCODE -ne 0) { throw 'WPF publish failed.' }
Copy-Item -Path (Join-Path $taskRoot 'tartarus/models') -Destination $out -Recurse -Force
Copy-Item -LiteralPath (Join-Path $taskRoot 'vorax/target/release/vorax-overlay.exe') -Destination $out -Force
Copy-Item -LiteralPath (Join-Path $taskRoot 'README.ko.md') -Destination (Join-Path $out 'README.ko.md') -Force
Copy-Item -LiteralPath (Join-Path $taskRoot 'VALIDATION.md') -Destination $out -Force
$licenses = Join-Path $out 'licenses'
New-Item -ItemType Directory -Path $licenses -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $taskRoot 'tartarus/LICENSE') -Destination (Join-Path $licenses 'TartarusSolver-MIT.txt') -Force
Copy-Item -LiteralPath (Join-Path $taskRoot 'vorax/LICENSE') -Destination (Join-Path $licenses 'VoraxSolver-MIT.txt') -Force
Copy-Item -Path (Join-Path $taskRoot 'licenses/*') -Destination $licenses -Force
Copy-Item -LiteralPath (Join-Path $taskRoot 'LICENSE') -Destination (Join-Path $licenses 'TorchlightOverlay-MIT.txt') -Force
Copy-Item -LiteralPath (Join-Path $taskRoot 'THIRD_PARTY_NOTICES.md') -Destination $out -Force
Write-Output "BUILD_OK: $out"
