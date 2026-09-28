param(
    [Parameter(Mandatory = $true)][string]$SourceDirectory,
    [Parameter(Mandatory = $true)][string]$DestinationPath
)

$ErrorActionPreference = 'Stop'
$source = [System.IO.Path]::GetFullPath($SourceDirectory)
$destination = [System.IO.Path]::GetFullPath($DestinationPath)
if (-not (Test-Path -LiteralPath $source -PathType Container)) {
    throw "Release staging directory was not found: $source"
}
$sourcePrefix = $source.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
if ($destination.Equals($source, [System.StringComparison]::OrdinalIgnoreCase) -or
    $destination.StartsWith($sourcePrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'The release archive cannot be written inside its source directory.'
}
if (Test-Path -LiteralPath $destination) {
    throw "Release archive already exists: $destination"
}
[System.IO.Directory]::CreateDirectory([System.IO.Path]::GetDirectoryName($destination)) | Out-Null

Add-Type -AssemblyName System.IO.Compression
$stream = [System.IO.File]::Open(
    $destination,
    [System.IO.FileMode]::CreateNew,
    [System.IO.FileAccess]::ReadWrite,
    [System.IO.FileShare]::None
)
$archive = [System.IO.Compression.ZipArchive]::new($stream, [System.IO.Compression.ZipArchiveMode]::Create, $false)
try {
    $files = [System.IO.Directory]::GetFiles($source, '*', [System.IO.SearchOption]::AllDirectories) | Sort-Object
    foreach ($file in $files) {
        $entryName = $file.Substring($sourcePrefix.Length).Replace('\', '/')
        if ([string]::IsNullOrWhiteSpace($entryName) -or $entryName.StartsWith('/')) {
            throw "Invalid release package path: $entryName"
        }
        $entry = $archive.CreateEntry($entryName, [System.IO.Compression.CompressionLevel]::Optimal)
        $entryStream = $entry.Open()
        $fileStream = [System.IO.File]::OpenRead($file)
        try {
            $fileStream.CopyTo($entryStream)
        }
        finally {
            $fileStream.Dispose()
            $entryStream.Dispose()
        }
    }
}
finally {
    $archive.Dispose()
    $stream.Dispose()
}
