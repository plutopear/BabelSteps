# Compact project status. Read-only.
$root = (git rev-parse --show-toplevel)
Set-Location $root

"== Git =="
"Branch: $(git branch --show-current)"
$dirty = @(git status --short)
"Uncommitted files: $($dirty.Count)"
$dirty | Select-Object -First 10
git log --oneline -5
$main = @(git branch --list main)
if ($main.Count -gt 0) {
    $unmerged = @(git branch --no-merged main --format "%(refname:short)")
    if ($unmerged.Count -gt 0) { "Not merged into main: $($unmerged -join ', ')" }
}

function Show-Matches($path, $pattern) {
    if (Test-Path $path) {
        Select-String -Path $path -Pattern $pattern | ForEach-Object { $_.Line }
    }
}

""
"== Milestones (docs/05-roadmap.md) =="
Show-Matches "docs/05-roadmap.md" '^## M\d|^\*\*Status'

""
"== Open questions (docs/06-open-questions.md) =="
Show-Matches "docs/06-open-questions.md" '^\*\*Open:' | ForEach-Object {
    if ($_.Length -gt 150) { $_.Substring(0, 150) + "..." } else { $_ }
}

""
"== M1 parity checklist (research/m1-parity-checklist.md) =="
if (Test-Path "research/m1-parity-checklist.md") {
    $lines = Get-Content "research/m1-parity-checklist.md" -Encoding UTF8
    $lines | Where-Object { $_ -like '**Where it stands*' } | Select-Object -First 1 |
        ForEach-Object { if ($_.Length -gt 220) { $_.Substring(0, 220) + "..." } else { $_ } }
    "Not yet passing:"
    $open = $lines | Where-Object { $_ -match '^\| [A-Z]+-?\d+ \|' } | ForEach-Object {
        $c = $_ -split '\|'
        $st = $c[-2].Trim()
        if ($st -ne 'Pass' -and $st -notlike 'Out of scope*') { "$($c[1].Trim()) ($st)" }
    }
    "  " + ($open -join ', ')
}
