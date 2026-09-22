# -*- mode: python ; coding: utf-8 -*-
import sys
from PyInstaller.utils.hooks import collect_data_files, collect_submodules, copy_metadata

datas = []
datas += collect_data_files("nicegui")
datas += collect_data_files("devicesim")
try:
    datas += copy_metadata("nicegui")
except Exception:
    pass

hiddenimports = []
hiddenimports += collect_submodules("paho")
hiddenimports += collect_submodules("pymodbus")
hiddenimports += collect_submodules("nicegui")
hiddenimports += [
    "pydantic",
    "uvicorn",
    "starlette",
    "watchfiles",
    "multipart",
    "docutils",
    "orjson",
]

block_cipher = None

a = Analysis(
    ["../src/devicesim/__main__.py"],
    pathex=["../src"],
    binaries=[],
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[],
    win_no_prefer_redirects=False,
    win_private_assemblies=False,
    cipher=block_cipher,
    noarchive=False,
)

pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="DeviceSim",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    console=False,
    disable_windowed_traceback=False,
    argv_emulation=sys.platform == "darwin",
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
    icon=None,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.zipfiles,
    a.datas,
    strip=False,
    upx=True,
    upx_exclude=[],
    name="DeviceSim",
)

if sys.platform == "darwin":
    app = BUNDLE(
        coll,
        name="DeviceSim.app",
        icon=None,
        bundle_identifier="com.devtools.devicesim",
        info_plist={
            "CFBundleName": "DeviceSim",
            "CFBundleDisplayName": "DeviceSim",
            "CFBundleShortVersionString": "1.0.0",
            "NSHighResolutionCapable": True,
        },
    )
