# Supersession, with the two shapes that must be refused: an object that
# supersedes itself, and a cycle back to the object it replaced.
$ErrorActionPreference = 'Continue'
$T = "D:\lukeo3o1\engr-audit\t7.ps1"
$G = "D:\lukeo3o1\engr-audit\g7.ps1"
$W = "r7/project"

Write-Output "===== the replacement object ====="
& $G -Log 15-supersession -Which current -WorkDir $W prepare --new --text "Continuity carried forward"
$listing = & $T -Log 15-supersession -Which current -WorkDir $W ls --all | Out-String
Write-Output $listing.TrimEnd()
$new = ($listing -split "`n" | Where-Object { $_ -match 'Continuity carried forward' }) -replace '^\s*(\S+).*', '$1'
$new = $new.Trim()
Write-Output "NEW OBJECT: $new"
$new | Set-Content -Encoding utf8 "D:\lukeo3o1\engr-audit\r7\evidence\replacement-object.txt"

Write-Output "===== the object about to be superseded needs a settled type ====="
& $G -Log 15-supersession -Which current -WorkDir $W prepare --classify --object 01a05e55-74 --type design --state proposed

Write-Output "===== an object cannot supersede itself ====="
& $T -Log 15-supersession -Which current -WorkDir $W prepare --supersede 01a05e55-74 --object 01a05e55-74 --text "a section that supersedes its own object"

Write-Output "===== the real supersession ====="
& $G -Log 15-supersession -Which current -WorkDir $W prepare --supersede $new --object 01a05e55-74 --text "The continuity question is settled; the replacement carries it forward." --header "Superseded"

Write-Output "===== and the cycle back ====="
& $G -Log 15-supersession -Which current -WorkDir $W prepare --classify --object $new --type decision --state accepted
& $T -Log 15-supersession -Which current -WorkDir $W prepare --supersede 01a05e55-74 --object $new --text "a cycle back to the object this one replaced"

Write-Output "===== where that leaves the workspace ====="
& $T -Log 15-supersession -Which current -WorkDir $W ls
& $T -Log 15-supersession -Which current -WorkDir $W verify
