# rtools Windows 安装包

先构建 Rust Engine 和 Avalonia 发布目录：

```powershell
cd crates/rtools-workspace
cargo build --release -p rtools-engine
cd ../../apps/desktop-avalonia
dotnet publish src/RTools.Desktop/RTools.Desktop.csproj -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -p:IncludeNativeLibrariesForSelfExtract=true -o ../../artifacts/desktop-win-x64
```

再使用 Inno Setup 编译：

```powershell
& "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" installer/rtools.iss
```

输出位于 `artifacts/release/rtools-0.2.0-setup.exe`。
