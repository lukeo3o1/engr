# Build the two comparison heads for run 7, each into its own CARGO_TARGET_DIR
# volume. Reusing a volume once finished in 0.25 s and copied out the *previous*
# head's binary at exit 0, silently; a fresh volume per head makes that
# impossible.
$ErrorActionPreference = 'Continue'
$pairs = @(
  @{ Sha = '9fba620'; Out = 'engr-prev';    Vol = 'engr-cargo-prev7' },
  @{ Sha = '9d7bf06'; Out = 'engr-current'; Vol = 'engr-cargo-cur7'  }
)
foreach ($build in $pairs) {
  Write-Output "=== building $($build.Sha) -> bin/$($build.Out) in volume $($build.Vol) ==="
  docker volume rm -f $build.Vol | Out-Null
  docker run --rm `
    -v "D:\lukeo3o1\engr-audit\src-$($build.Sha):/src" `
    -v "$($build.Vol):/target" `
    -v "engr-cargo-registry:/usr/local/cargo/registry" `
    -e CARGO_TARGET_DIR=/target `
    -w /src `
    engr-rust:latest `
    bash -c "cargo build --release -p engr 2>&1 | tail -5 && ls -l /target/release/engr && sha256sum /target/release/engr"
  if ($LASTEXITCODE -ne 0) { Write-Output "BUILD FAILED for $($build.Sha)"; exit 1 }
  docker run --rm `
    -v "$($build.Vol):/target" `
    -v "D:\lukeo3o1\engr-audit:/audit" `
    engr-rust:latest `
    bash -c "cp /target/release/engr /audit/bin/$($build.Out) && sha256sum /audit/bin/$($build.Out)"
  if ($LASTEXITCODE -ne 0) { Write-Output "COPY-OUT FAILED for $($build.Sha)"; exit 1 }
}
Write-Output "=== both built ==="
