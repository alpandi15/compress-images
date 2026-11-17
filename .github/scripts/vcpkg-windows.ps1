# Installs static libheif + dav1d for x64 MSVC with the runner's vcpkg and exports the
# variables the build scripts read (SPEC §7.3). Used by ci.yml and release.yml.
$ErrorActionPreference = "Stop"
$triplet = "x64-windows-static-md"   # static libraries, dynamic CRT (what Rust links)
$root = $env:VCPKG_INSTALLATION_ROOT
New-Item -ItemType Directory -Force $env:VCPKG_DEFAULT_BINARY_CACHE | Out-Null

& "$root\vcpkg.exe" install --triplet $triplet --clean-after-build libheif dav1d
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
choco install pkgconfiglite -y --no-progress
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$installed = "$root\installed\$triplet"
@(
  "VCPKG_ROOT=$root",                         # libheif-sys finds libheif through vcpkg
  "VCPKGRS_TRIPLET=$triplet",
  "PKG_CONFIG_PATH=$installed\lib\pkgconfig", # dav1d-sys finds dav1d through pkg-config
  "SYSTEM_DEPS_DAV1D_LINK=static"
) | Add-Content $env:GITHUB_ENV
