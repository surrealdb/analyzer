-- Organization side of the corpus: a nested object field, an untyped `array`
-- refined per-element via `[*]` (exercises `[*]` projection — 3.2.3 rejects
-- the same refinement under a parent typed `array<object>`, see `contacts`
-- below), `array<record<…>>` links, and `COMPUTED <~relation`
-- reverse-reference fields.

DEFINE TABLE sector SCHEMAFULL;

DEFINE FIELD name ON sector TYPE string;

DEFINE TABLE organization SCHEMAFULL;

DEFINE FIELD name ON organization TYPE string;

DEFINE FIELD legal_name ON organization TYPE string;

DEFINE FIELD logo ON organization TYPE option<string>;

DEFINE FIELD tier ON organization TYPE 'free' | 'pro' | 'enterprise' DEFAULT 'free';

DEFINE FIELD parent ON organization TYPE option<record<organization>> REFERENCE;

DEFINE FIELD owner ON organization TYPE record<account>;

DEFINE FIELD sectors ON organization TYPE array<record<sector>> DEFAULT [];

-- Untyped `array`, not `array<object>`: 3.2.3 refuses the combination —
-- `Cannot set field `contacts.*` with type `{ email: string, label: string,
-- primary: bool }` as it mismatched with field `contacts` with type
-- `array<object>`` — so the per-element shape below is the only place the
-- element type is declared.
DEFINE FIELD contacts ON organization TYPE array DEFAULT [];

DEFINE FIELD contacts[*] ON organization TYPE {
	label: string,
	email: string,
	primary: bool
};

DEFINE FIELD address ON organization TYPE {
	line1: string,
	line2: option<string>,
	city: string,
	country: string
};

DEFINE FIELD employees ON organization COMPUTED <~employee_of;

DEFINE FIELD subsidiaries ON organization COMPUTED <~organization;

DEFINE FIELD created_at ON organization TYPE datetime DEFAULT time::now();

DEFINE TABLE organization_unit SCHEMAFULL;

DEFINE FIELD name ON organization_unit TYPE string;

DEFINE FIELD organization ON organization_unit TYPE record<organization>;

DEFINE FIELD parent ON organization_unit TYPE option<record<organization_unit>>;

DEFINE FIELD headcount ON organization_unit TYPE int DEFAULT 0;

DEFINE TABLE organization_settings SCHEMAFULL;

DEFINE FIELD organization ON organization_settings TYPE record<organization>;

DEFINE FIELD security ON organization_settings TYPE {
	byok: {
		mode: 'lazy' | 'immediate',
		status: 'not_started' | 'running' | 'completed' | 'failed',
		progress: int,
		kms_provider: option<string>
	}
};

-- A field's own `DEFAULT`/`VALUE` is checked against its declared type under
-- the same contract a *written* value is: a constant must be a member of the
-- union (`DEFAULT 45` here is the same E2001 as `SET retention_days = 45`),
-- while a value that is not statically known — this `string::lowercase` call —
-- can never be proven to fall outside it and stays silent.
DEFINE FIELD retention_days ON organization_settings TYPE 30 | 90 | 365 DEFAULT 30;

DEFINE FIELD data_region ON organization_settings TYPE 'us' | 'eu' VALUE string::lowercase($value);

DEFINE FIELD updated_at ON organization_settings TYPE datetime VALUE time::now();
