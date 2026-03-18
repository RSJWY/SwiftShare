# SwiftShare Usage Guide

This guide will help you get started with SwiftShare for high-speed file sharing on your local network.

---

## 1. 🚀 Launch the Application

- Double-click `SwiftShare.exe` to run. Developers can also use `pnpm tauri dev` in the source directory.
- On first launch, the app automatically generates a unique machine ID for device identification.
- The interface is simple: shared files area at the top, device list on the bottom left, remote file browser in the middle, and transfer progress on the right.

---

## 2. 📡 Device Discovery

- While the app is running, it automatically discovers other SwiftShare devices on your LAN via mDNS protocol.
- The device list shows device names, IP addresses, and port numbers.
- Click the refresh button to force a rescan if you don't see a new device.
- Scanning typically completes within 3-5 seconds.
- **Tip**: If devices aren't appearing, check if your firewall is blocking the app's network access.

---

## 3. 📁 Share Files (Drag & Drop)

- To share files, simply drag them to the "Shared Files" area.
- Single files, multiple files, and entire folders are all supported.
- Added items appear immediately in the list.
- All devices on your LAN can see and access these files.
- Click "Clear" to remove all shared files at once.

---

## 4. 📂 Browse Remote Device Files

- Click on a device in the "Online Devices" list to view its shared content.
- The middle area displays all files shared by that device.
- Click folders to navigate into subdirectories.
- Use the breadcrumb navigation at the top to go back to parent directories.

---

## 5. 📥 Pull Files (Pull Mode)

- While browsing remote files, click the "Pull" button next to a file or folder.
- The file will download to your preset local directory.
- Supports downloading single files or entire folders.
- **Note**: Make sure you've set a download directory before starting downloads.

---

## 6. 🖱️ Drag Out Files (Drag-out Mode)

- This is a more efficient way to save files: hold the "Drag" button next to a remote file.
- Drag it directly to a target folder on your computer (like Desktop or File Explorer).
- Release to start downloading to that location.
- Perfect for quickly saving files to specific destinations.

---

## 7. 📊 Transfer Progress

- During transfers, a progress ring and status are displayed.
- View real-time transfer speed and estimated time remaining.
- The last 20 transfers are kept in history.
- Click cancel anytime to interrupt an ongoing transfer.

---

## 8. ⚠️ Conflict Handling

- If a file with the same name already exists in the target directory, a conflict dialog appears.
- The dialog shows the list of conflicting files and total size.
- Choose to continue (overwrite) or cancel the transfer.

---

## Related Documentation

- [Settings Reference](./settings.md) - Learn about each configuration option
- [Troubleshooting](./troubleshooting.md) - Having issues? Find solutions here
