// The public-data publishers by region, shared by the Market Data tab and the
// supported-files dialog so the two cannot group them differently.

/**
 * The publishers by region, as Xfina Labs groups its data: India, US, Global,
 * and Reference for rates and inflation. A publisher sits where Labs uses it:
 * Tiingo and Yahoo for US-listed funds and indices, WSJ for gold funds listed
 * outside the US, iShares for its US and UCITS funds alike. Labs' other
 * grouping, by asset class, cannot be read off a publisher -- AMFI, NSE and
 * Tiingo each carry equity, debt and gold -- nor off the file.
 *
 * Presentation only: the library knows a format's category, not its region,
 * so a format added without one here lands in "Other" rather than going
 * missing.
 */
export const PUBLISHER_GROUPS = [
    { label: 'India', ids: ['md-amfi-nav', 'md-nse-security', 'md-nse-indices', 'md-mcx-spot'] },
    { label: 'US', ids: ['md-nasdaq', 'md-spdr-gold', 'md-tiingo', 'md-yahoo'] },
    { label: 'Global', ids: ['md-msci', 'md-ishares', 'md-wsj'] },
    { label: 'Reference', ids: ['rt-sbi-forex-card'] },
];

/** The library's public formats, under these groups, in this order. */
export const groupPublicFormats = (formats) => {
    const publicFormats = formats.filter(f => f.area === 'public');
    const byId = new Map(publicFormats.map(f => [f.id, f]));
    const grouped = PUBLISHER_GROUPS
        .map(g => ({ label: g.label, entries: g.ids.map(id => byId.get(id)).filter(Boolean) }))
        .filter(g => g.entries.length);
    const placed = new Set(PUBLISHER_GROUPS.flatMap(g => g.ids));
    const other = publicFormats.filter(f => !placed.has(f.id));
    return other.length ? [...grouped, { label: 'Other', entries: other }] : grouped;
};
