# Run the accumulated container-side suites, each to its own evidence file.
#
# The crash sequence is NOT here. crash-sweep.sh rebuilds crash-runs/, and both
# crash-resume.sh and crash-objects-identical.sh read what it made; run from two
# places they report on whichever subset survived. crash8.ps1 owns all three.
$ErrorActionPreference = 'Continue'
$A = "D:\lukeo3o1\engr-audit"
$jobs = @(
  @{ S = 'adversarial.sh';             O = 'evidence\adversarial.txt' },
  @{ S = 'adv-yaml.sh';                O = 'evidence\adv-yaml.txt' },
  @{ S = 'adv-git.sh';                 O = 'evidence\adv-git.txt' },
  @{ S = 'adv-final.sh';               O = 'evidence\adv-final.txt' },
  @{ S = 'adv-verify.sh';              O = 'evidence\adv-verify-vs-show.txt' },
  @{ S = 'withdrawal.sh';              O = 'evidence\withdrawal.txt' },
  @{ S = 'withdraw-interrupt.sh';      O = 'evidence\withdraw-interrupt.txt' },
  @{ S = 'withdraw-stale.sh';          O = 'evidence\withdraw-stale.txt' },
  @{ S = 'source-moved.sh';            O = 'evidence\source-moved.txt' },
  @{ S = 'nested-exclusion.sh';        O = 'evidence\nested-exclusion.txt' },
  @{ S = 'publication-order.sh';       O = 'evidence\publication-order.txt' },
  @{ S = 'barrier-window.sh';          O = 'evidence\barrier-window.txt' },
  @{ S = 'barrier-switch.sh';          O = 'evidence\barrier-switch.txt' },
  @{ S = 'stale-stage.sh';             O = 'evidence\stale-stage.txt' },
  @{ S = 'stale-stage2.sh';            O = 'evidence\stale-stage2.txt' },
  @{ S = 'tail-and-purge.sh';          O = 'evidence\tail-and-purge.txt' },
  @{ S = 'crash-tail-ref.sh';          O = 'evidence\crash-tail-ref.txt' },
  @{ S = 'new-surfaces.sh';            O = 'evidence\new-surfaces.txt' },
  @{ S = 'content-flags.sh';           O = 'evidence\content-flags.txt' }
)
foreach ($job in $jobs) {
  Write-Output "######## $($job.S) ########"
  & "$A\s8.ps1" -Script $job.S -Out $job.O | Out-Null
  Write-Output "  -> $($job.O)"
}
Write-Output "######## batch done ########"
