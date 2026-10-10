param([string]$Destination = (Join-Path $PSScriptRoot '../tools/7zip'))
$ErrorActionPreference = 'Stop'
$version = '26.04'
$expectedMsiHash = '0b01334a418654293513449f61d6bfc99e5196ca371e3c3dc961eda57cd535c6'
$sourceUrl = 'https://github.com/ip7z/7zip/releases/download/26.04/7z2604-x64.msi'
$destinationPath = [IO.Path]::GetFullPath($Destination)
$temporaryPath = Join-Path ([IO.Path]::GetTempPath()) ('OpenUnpack-engine-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temporaryPath -Force | Out-Null
New-Item -ItemType Directory -Path $destinationPath -Force | Out-Null

# Read the installer as a database. This does not install 7-Zip or change the registry.
if (-not ('OpenUnpackMsiReader' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Collections.Generic;
public static class OpenUnpackMsiReader {
    [DllImport("msi.dll", CharSet=CharSet.Unicode)] static extern uint MsiOpenDatabase(string path, IntPtr mode, out uint database);
    [DllImport("msi.dll", CharSet=CharSet.Unicode)] static extern uint MsiDatabaseOpenView(uint database, string query, out uint view);
    [DllImport("msi.dll")] static extern uint MsiViewExecute(uint view, uint record);
    [DllImport("msi.dll")] static extern uint MsiViewFetch(uint view, out uint record);
    [DllImport("msi.dll", CharSet=CharSet.Unicode)] static extern uint MsiRecordGetString(uint record, uint field, StringBuilder value, ref uint length);
    [DllImport("msi.dll")] static extern uint MsiRecordReadStream(uint record, uint field, byte[] data, ref uint size);
    [DllImport("msi.dll")] static extern uint MsiCloseHandle(uint handle);
    static void Check(uint error) { if (error != 0) throw new IOException("MSI read error: " + error); }
    static string GetString(uint record, uint field) { uint length=4096; var text=new StringBuilder((int)length); Check(MsiRecordGetString(record, field, text, ref length)); return text.ToString(); }
    public static Dictionary<string,string> ExtractCabinets(string path, string destination) {
        uint database=0, view=0, record=0;
        var files=new Dictionary<string,string>(StringComparer.OrdinalIgnoreCase);
        try {
            Check(MsiOpenDatabase(path, IntPtr.Zero, out database));
            Check(MsiDatabaseOpenView(database, "SELECT `File`, `FileName` FROM `File`", out view));
            Check(MsiViewExecute(view, 0));
            while (MsiViewFetch(view, out record)==0) {
                var name=GetString(record,2); int split=name.IndexOf('|'); if(split>=0) name=name.Substring(split+1);
                files[name]=GetString(record,1); MsiCloseHandle(record); record=0;
            }
            MsiCloseHandle(view); view=0;
            Check(MsiDatabaseOpenView(database, "SELECT `Name`, `Data` FROM `_Streams`", out view));
            Check(MsiViewExecute(view, 0));
            int cabinetNumber=0;
            while(MsiViewFetch(view,out record)==0) {
                var buffer=new byte[65536]; uint count=(uint)buffer.Length; Check(MsiRecordReadStream(record,2,buffer,ref count));
                if(count>=4 && buffer[0]==77 && buffer[1]==83 && buffer[2]==67 && buffer[3]==70) {
                    using(var output=File.Create(Path.Combine(destination, "payload-"+(cabinetNumber++)+".cab"))) {
                        output.Write(buffer,0,(int)count);
                        while(count>0) { count=(uint)buffer.Length; Check(MsiRecordReadStream(record,2,buffer,ref count)); output.Write(buffer,0,(int)count); }
                    }
                }
                MsiCloseHandle(record); record=0;
            }
            if(cabinetNumber==0) throw new IOException("No embedded cabinet found in official MSI.");
            return files;
        } finally { if(record!=0) MsiCloseHandle(record); if(view!=0) MsiCloseHandle(view); if(database!=0) MsiCloseHandle(database); }
    }
}
'@
}

try {
    $msiPath = Join-Path $temporaryPath '7z2604-x64.msi'
    Write-Host "Downloading official 7-Zip $version..."
    Invoke-WebRequest -Uri $sourceUrl -OutFile $msiPath
    $actualMsiHash = (Get-FileHash -LiteralPath $msiPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualMsiHash -ne $expectedMsiHash) { throw 'Official 7-Zip installer SHA256 does not match the pinned release.' }
    $files = [OpenUnpackMsiReader]::ExtractCabinets($msiPath, $temporaryPath)
    $expandedPath = Join-Path $temporaryPath 'expanded'
    New-Item -ItemType Directory -Path $expandedPath | Out-Null
    foreach ($cabinet in Get-ChildItem -LiteralPath $temporaryPath -Filter '*.cab') {
        & "$env:SystemRoot/System32/expand.exe" '-F:*' $cabinet.FullName $expandedPath | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Failed to expand official MSI cabinet.' }
    }
    foreach ($fileName in @('7z.dll', '7z.exe')) {
        if (-not $files.ContainsKey($fileName)) { throw "Official MSI is missing $fileName." }
        $payloadPath = Join-Path $expandedPath $files[$fileName]
        Copy-Item -LiteralPath $payloadPath -Destination (Join-Path $destinationPath $fileName) -Force
    }
    $licencePath = Join-Path $destinationPath 'License.txt'
    if ($files.ContainsKey('License.txt')) {
        Copy-Item -LiteralPath (Join-Path $expandedPath $files['License.txt']) -Destination $licencePath -Force
    } else { Invoke-WebRequest -Uri 'https://www.7-zip.org/license.txt' -OutFile $licencePath }
    $dllHash=(Get-FileHash -LiteralPath (Join-Path $destinationPath '7z.dll') -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText((Join-Path $destinationPath '7z.dll.sha256'), "$dllHash  7z.dll`n", [Text.UTF8Encoding]::new($false))
    @{ version=$version; source=$sourceUrl; msiSha256=$expectedMsiHash; dllSha256=$dllHash; license='https://www.7-zip.org/license.txt' } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $destinationPath 'provenance.json') -Encoding utf8NoBOM
    Write-Host "Official 7-Zip $version ready at $destinationPath"
} finally {
    # No recursive deletion: the temporary installer and cabinet remain available for inspection.
}
