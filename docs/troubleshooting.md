# Troubleshooting & FAQ

## 1. Device Discovery 🔍

### Q: Can't see other devices?
**Possible causes:**
1. **Firewall blocking** - Windows Firewall may block mDNS broadcasts
   - Solution: Allow SwiftShare through the firewall, or add an inbound rule for UDP port 5353
2. **Not on the same LAN** - Devices must be connected to the same router
   - Solution: Verify both devices are on the same network
3. **Virtual network interference** - VPNs or virtual network adapters may interfere
   - Solution: Disable VPN, or enable "Same Subnet Only" in settings
4. **Network isolation** - Some corporate/school networks block device-to-device communication
   - Solution: Contact your network administrator

### Q: Device appears multiple times?
**Cause:** Computer has multiple network adapters (WiFi + Ethernet + virtual adapters)
**Status:** Optimized - each machine shows once, automatically selecting the best IP
**Solution:** If issues persist, enable "Same Subnet Only" in settings

---

## 2. File Transfer 🚀

### Q: Transfer stuck or not progressing?
**Possible causes:**
1. Unstable network connection
2. Remote device went to sleep or disconnected
3. File is locked by another program

**Solutions:**
- Check your network connection status
- Cancel and retry the transfer
- Verify the file isn't locked by another application

### Q: Transfer speed is slow?
**Possible causes:**
1. Weak WiFi signal
2. Network congestion from other devices
3. Speed limit setting is too low

**Solutions:**
- Move closer to the router or use wired connection
- Pause other downloads/uploads
- Check the "Speed Limit" setting

### Q: Pull file failed?
**Possible causes:**
1. Download directory doesn't exist or lacks write permission
2. Insufficient disk space
3. Filename contains special characters

**Solutions:**
- Choose a different download directory
- Check available disk space
- Try renaming the file

---

## 3. Conflict Handling ⚠️

### Q: Shows "File exists" conflict?
**Explanation:** A file with the same name already exists in the target directory
**Options:**
- Click "Continue" to overwrite the existing file
- Click "Cancel" to abort the transfer

---

## 4. Application Issues ⚙️

### Q: Settings don't take effect after saving?
- Some settings require an app restart to apply

### Q: Update check fails?
**Possible causes:**
1. GitHub access is restricted in your region
2. Update mirror URL is invalid

**Solutions:**
- Check that the "Update Mirror" setting is correct
- China users: use the default mirror
- International users: leave the mirror URL empty

### Q: How to completely clear all data?
- Delete the following directory to reset the app:
  - Windows: `%APPDATA%\SwiftShare\`
- This includes `.machine_id` file and `settings.json`
