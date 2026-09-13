-- 5xxx — functions and closures. One intended violation per line.

-- 5001: a call to a function that does not exist — builtin, fn::, and method.
RETURN ghost::function(1);
RETURN fn::nope::missing();
RETURN 'text'.nosuchmethod();

-- 5002: a call that does not match the signature.
RETURN string::len(1, 2);
RETURN fn::org::display_name('not a record');

-- 5002: `rand::int`/`rand::float`/`rand::time` take 0 or 2 arguments, not a
-- 0..=2 range; `rand::duration` requires both bounds.
RETURN rand::int(100);
RETURN rand::float(1.0);
RETURN rand::time(1000);
RETURN rand::duration();

-- 5002: `record::id`/`record::tb`/`record::table` require a record argument.
RETURN record::id('ann');
RETURN record::tb('ann');
RETURN record::table('ann');

-- 5002: `type::table` takes a string or a record, not a bare number.
RETURN type::table(30);

-- 5005: a constant argument that fails the function's value contract.
SELECT type::field('nope') FROM article WHERE id = article:hello;

-- 5009: a fn:: definition that never terminates.
DEFINE FUNCTION fn::bad::forever() { RETURN fn::bad::forever(); };

-- 5010: an event that triggers itself.
DEFINE EVENT article_self_touch ON article THEN { UPDATE article SET views += 1; };
