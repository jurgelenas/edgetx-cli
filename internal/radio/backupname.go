package radio

import "time"

// BackupDirName returns a backup directory name with the current date appended.
// If prefix is empty, it defaults to "backup".
func BackupDirName(prefix string) string {
	dateSuffix := time.Now().Format("2006-01-02")
	if prefix == "" {
		return "backup-" + dateSuffix
	}
	return prefix + "-" + dateSuffix
}
