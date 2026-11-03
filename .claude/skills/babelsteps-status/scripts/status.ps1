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
"== Known test gaps (research/m0-spike-results.md) =="
if (Test-Path "research/m0-spike-results.md") {
    $lines = Get-Content "research/m0-spike-results.md"
    $start = [array]::IndexOf($lines, "## Not tested")
    if ($start -ge 0) {
        $lines[($start + 1)..($lines.Count - 1)] |
            Where-Object { $_ -match '^\| ' -and $_ -notmatch '^\| (Gap|---)' } |
            ForEach-Object { "- " + ($_ -split '\|')[1].Trim() }
    }
}
