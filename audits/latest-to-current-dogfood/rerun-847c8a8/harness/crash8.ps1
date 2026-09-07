# The whole crash sequence in one go, because each step reads the runs the one
# before it made: sweep, resume every instant, then compare every resumed
# workspace's objects against the migrated checkpoint.
$ErrorActionPreference = 'Continue'
$A = "D:\lukeo3o1\engr-audit"
$instants = @(500,600,700,800,850,900,930,950,980,1000,1020,1040,1060,1080,1090,1100,1110,1120)
& "$A\s8.ps1" -Script crash-sweep.sh -Out "evidence\crash-sweep.txt" @instants | Out-Null
Write-Output "sweep done"
& "$A\s8.ps1" -Script crash-resume.sh -Out "evidence\crash-resume.txt" @instants | Out-Null
Write-Output "resume done"
& "$A\s8.ps1" -Script crash-objects-identical.sh -Out "evidence\crash-objects-identical.txt" | Out-Null
Write-Output "identity check done"
