package simulator

import (
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func sampleCatalog() []RadioDef {
	return []RadioDef{
		{
			Name: "RadioMaster TX16S",
			WASM: "tx16s.wasm",
			Display: DisplayDef{W: 480, H: 272, Depth: 16},
			Keys: []KeyDef{
				{Key: "SYS", Label: "SYS", Side: "L"},
				{Key: "ENTER", Label: "ENT", Side: "R"},
			},
			Switches: []SwitchDef{
				{Name: "SA", Type: "3POS", Default: "3POS"},
				{Name: "SB", Type: "3POS", Default: "3POS"},
			},
			Trims: []TrimDef{
				{Name: "T1"}, {Name: "T2"}, {Name: "T3"}, {Name: "T4"},
			},
		},
		{
			Name: "Jumper T-Pro",
			WASM: "t-pro.wasm",
			Display: DisplayDef{W: 128, H: 64, Depth: 1},
		},
		{
			Name: "FrSky Horus X10S",
			WASM: "x10s.wasm",
			Display: DisplayDef{W: 480, H: 272, Depth: 16},
		},
		{
			Name: "Flysky EL18",
			WASM: "el18.wasm",
			Display: DisplayDef{W: 320, H: 480, Depth: 16},
		},
	}
}

func TestFindRadio_ExactName(t *testing.T) {
	catalog := sampleCatalog()

	r, err := FindRadio(catalog, "RadioMaster TX16S")
	require.NoError(t, err)
	assert.Equal(t, "RadioMaster TX16S", r.Name)
}

func TestFindRadio_CaseInsensitive(t *testing.T) {
	catalog := sampleCatalog()

	r, err := FindRadio(catalog, "radiomaster tx16s")
	require.NoError(t, err)
	assert.Equal(t, "RadioMaster TX16S", r.Name)
}

func TestFindRadio_ByKey(t *testing.T) {
	catalog := sampleCatalog()

	r, err := FindRadio(catalog, "radiomaster-tx16s")
	require.NoError(t, err)
	assert.Equal(t, "RadioMaster TX16S", r.Name)
}

func TestFindRadio_ByWASMSlug(t *testing.T) {
	catalog := sampleCatalog()

	r, err := FindRadio(catalog, "tx16s")
	require.NoError(t, err)
	assert.Equal(t, "RadioMaster TX16S", r.Name)
}

func TestFindRadio_SubstringMatch(t *testing.T) {
	catalog := sampleCatalog()

	r, err := FindRadio(catalog, "EL18")
	require.NoError(t, err)
	assert.Equal(t, "Flysky EL18", r.Name)
}

func TestFindRadio_NotFound(t *testing.T) {
	catalog := sampleCatalog()

	_, err := FindRadio(catalog, "nonexistent")
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "no radio found")
}

func TestFindRadio_Ambiguous(t *testing.T) {
	catalog := sampleCatalog()

	// "s" matches both "RadioMaster TX16S" and "FrSky Horus X10S"
	_, err := FindRadio(catalog, "S")
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "ambiguous")
}

func TestRadioDefKey(t *testing.T) {
	r := RadioDef{Name: "RadioMaster TX16S (MKII)"}
	assert.Equal(t, "radiomaster-tx16s-mkii", r.Key())
}
