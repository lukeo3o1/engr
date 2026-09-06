# Type and state across all three vocabularies, with the invalid pairs refused.
# Every legal move goes through the gate; every illegal one must not reach it.
$ErrorActionPreference = 'Continue'
$T = "D:\lukeo3o1\engr-audit\t5.ps1"
$G = "D:\lukeo3o1\engr-audit\g5.ps1"
$W = "r5/project"
$O = "01a05e55-ee"

Write-Output "===== an invalid type/state pair must not reach the gate ====="
& $T -Log 14-lifecycle -Which current -WorkDir $W prepare --object $O --classify --type decision --state identified

Write-Output "===== the three vocabularies, each legal move confirmed ====="
foreach ($p in @(@('design','draft'), @('design','accepted'), @('risk','identified'), @('risk','mitigated'))) {
  & $G -Log 14-lifecycle -Which current -WorkDir $W prepare --object $O --classify --type $p[0] --state $p[1]
}

Write-Output "===== back to untyped: an accepted untyped object is not a thing ====="
& $T -Log 14-lifecycle -Which current -WorkDir $W prepare --object $O --classify --untyped --state accepted
& $G -Log 14-lifecycle -Which current -WorkDir $W prepare --object $O --classify --untyped --state open

Write-Output "===== close, and closing what is already closed ====="
& $G -Log 14-lifecycle -Which current -WorkDir $W prepare --object $O --close
& $T -Log 14-lifecycle -Which current -WorkDir $W prepare --object $O --close
