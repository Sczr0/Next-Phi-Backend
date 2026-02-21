Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$featureRoot = Join-Path $repoRoot "src/features"

if (-not (Test-Path $featureRoot)) {
    Write-Error "[Stage2] feature root not found: $featureRoot"
    exit 2
}

$handlers = Get-ChildItem -Path $featureRoot -Recurse -Filter "handler.rs" -File
$pathPattern = [regex]"src[\\/]+features[\\/]+([^\\/]+)[\\/]+handler\.rs$"
$refPattern = [regex]"crate::features::([a-z_]+)::handler\b"

$violations = New-Object System.Collections.Generic.List[object]

foreach ($file in $handlers) {
    $normalizedFullPath = $file.FullName -replace "\\", "/"
    $pathMatch = $pathPattern.Match($normalizedFullPath)
    if (-not $pathMatch.Success) {
        continue
    }

    $sourceFeature = $pathMatch.Groups[1].Value
    $lines = Get-Content -Path $file.FullName
    $relativePath = $file.FullName.Substring($repoRoot.Length).TrimStart('\', '/') -replace "\\", "/"

    for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]
        $matches = $refPattern.Matches($line)
        foreach ($m in $matches) {
            $targetFeature = $m.Groups[1].Value
            if ($targetFeature -eq $sourceFeature) {
                continue
            }

            $violations.Add([PSCustomObject]@{
                Source = $sourceFeature
                Target = $targetFeature
                File   = $relativePath
                Line   = $i + 1
                Text   = $line.Trim()
            })
        }
    }
}

if ($violations.Count -gt 0) {
    Write-Host "[Stage2] FAIL: cross-feature handler dependencies found: $($violations.Count)" -ForegroundColor Red
    foreach ($v in $violations) {
        Write-Host (" - {0}:{1} [{2} -> {3}] {4}" -f $v.File, $v.Line, $v.Source, $v.Target, $v.Text)
    }
    exit 1
}

Write-Host "[Stage2] PASS: no cross-feature handler dependency. scanned handler files: $($handlers.Count)" -ForegroundColor Green
exit 0
