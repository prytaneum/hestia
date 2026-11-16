use std::collections::HashMap;

use super::bindings::hestia::kernel::messages;
use super::host::State;

const KEY_BYTES: usize = 128;
const ARGS: usize = 16;
const ARG_NAME_BYTES: usize = 64;
const ARG_VALUE_BYTES: usize = 1024;
const TEXT_BYTES: usize = 4096;

impl messages::Host for State {
    fn text(&mut self, key: String, args: Vec<(String, String)>) -> String {
        let key = clip(&key, KEY_BYTES);
        let template = self
            .shared
            .services
            .message(&self.plugin, key)
            .unwrap_or_else(|| key.to_owned());
        let args: HashMap<&str, &str> = args
            .iter()
            .take(ARGS)
            .filter(|(name, _)| name.len() <= ARG_NAME_BYTES)
            .map(|(name, value)| (name.as_str(), clip(value, ARG_VALUE_BYTES)))
            .collect();
        render(&template, &args)
    }
}

fn clip(text: &str, bytes: usize) -> &str {
    &text[..text.floor_char_boundary(bytes)]
}

/// One pass over the template, so a value is never expanded again by a later
/// argument, stopped at [`TEXT_BYTES`]; the guest's input never grows host
/// memory past that.
fn render(template: &str, args: &HashMap<&str, &str>) -> String {
    let mut out = String::new();
    let mut rest = template;
    while out.len() < TEXT_BYTES {
        let Some(open) = rest.find('{') else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after
            .find('}')
            .and_then(|close| Some((args.get(&after[..close])?, close)))
        {
            Some((value, close)) => {
                out.push_str(value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    clip(&out, TEXT_BYTES).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_expands_once_and_never_past_its_cap() {
        let args = HashMap::from([("a", "{b}"), ("b", "x")]);
        assert_eq!(render("{a} and {b} and {c}", &args), "{b} and x and {c}");
        let bomb = "{a}".repeat(1_000_000);
        let big = "y".repeat(ARG_VALUE_BYTES);
        let args = HashMap::from([("a", big.as_str())]);
        assert!(render(&bomb, &args).len() <= TEXT_BYTES);
    }
}
