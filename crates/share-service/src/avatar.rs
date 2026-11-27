//! A maker's block head: picked from a few colours, eyes and tops on the
//! web, drawn as SVG. Nobody uploads a picture, so there's nothing to check.

pub const COLOURS: &[&str] = &["e0663c", "f2b134", "6dbb4c", "23a88f", "3d8fd6", "7b55d6", "d65a9b", "8a5a3c", "f1d3b3", "5b6573"];
pub const EYES: &[&str] = &["dots", "happy", "sleepy", "wide", "wink"];
pub const TOPS: &[&str] = &["none", "tuft", "cap", "leaf", "bow"];

#[derive(Clone, Debug, PartialEq)]
pub struct Avatar {
    pub head: String,
    pub eyes: String,
    pub top: String,
    pub top_colour: String,
}

impl Avatar {
    /// A maker's own head before they choose: picked from their handle.
    pub fn default_for(handle: &str) -> Avatar {
        let h = handle.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
        Avatar {
            head: COLOURS[(h % COLOURS.len() as u32) as usize].into(),
            eyes: EYES[((h / 7) % EYES.len() as u32) as usize].into(),
            top: TOPS[((h / 53) % TOPS.len() as u32) as usize].into(),
            top_colour: COLOURS[((h / 401) % COLOURS.len() as u32) as usize].into(),
        }
    }

    pub fn parse(s: &str, handle: &str) -> Avatar {
        let mut a = Avatar::default_for(handle);
        for part in s.split(';') {
            let Some((k, v)) = part.split_once('=') else { continue };
            match k {
                "h" if COLOURS.contains(&v) => a.head = v.into(),
                "e" if EYES.contains(&v) => a.eyes = v.into(),
                "t" if TOPS.contains(&v) => a.top = v.into(),
                "c" if COLOURS.contains(&v) => a.top_colour = v.into(),
                _ => {}
            }
        }
        a
    }

    /// Only the allowed choices; anything else comes back as None.
    pub fn from_choices(head: &str, eyes: &str, top: &str, top_colour: &str) -> Option<Avatar> {
        (COLOURS.contains(&head) && EYES.contains(&eyes) && TOPS.contains(&top) && COLOURS.contains(&top_colour))
            .then(|| Avatar { head: head.into(), eyes: eyes.into(), top: top.into(), top_colour: top_colour.into() })
    }

    pub fn encode(&self) -> String {
        format!("h={};e={};t={};c={}", self.head, self.eyes, self.top, self.top_colour)
    }

    pub fn svg(&self) -> String {
        let eyes = match self.eyes.as_str() {
            "happy" => r##"<path d="M19 30q4-5 8 0M37 30q4-5 8 0" stroke="#1d2430" stroke-width="3" fill="none" stroke-linecap="round"/>"##.to_string(),
            "sleepy" => r##"<path d="M18 30h9M37 30h9" stroke="#1d2430" stroke-width="3" stroke-linecap="round"/>"##.to_string(),
            "wide" => r##"<rect x="18" y="24" width="10" height="10" rx="2" fill="#fff"/><rect x="36" y="24" width="10" height="10" rx="2" fill="#fff"/><rect x="21" y="27" width="5" height="5" fill="#1d2430"/><rect x="39" y="27" width="5" height="5" fill="#1d2430"/>"##.to_string(),
            "wink" => r##"<rect x="20" y="25" width="6" height="7" rx="1" fill="#1d2430"/><path d="M37 30q4-5 8 0" stroke="#1d2430" stroke-width="3" fill="none" stroke-linecap="round"/>"##.to_string(),
            _ => r##"<rect x="20" y="25" width="6" height="7" rx="1" fill="#1d2430"/><rect x="38" y="25" width="6" height="7" rx="1" fill="#1d2430"/>"##.to_string(),
        };
        let c = &self.top_colour;
        let top = match self.top.as_str() {
            "tuft" => format!(r##"<path d="M24 12l4-8 4 6 4-7 4 9z" fill="#{c}"/>"##),
            "cap" => format!(r##"<path d="M10 16q22-14 44 0v4H10z" fill="#{c}"/><rect x="40" y="16" width="18" height="4" rx="2" fill="#{c}"/>"##),
            "leaf" => format!(r##"<path d="M32 13q-2-9 8-11q0 10-8 11z" fill="#{c}"/><path d="M32 14v-4" stroke="#5a3c1e" stroke-width="2"/>"##),
            "bow" => format!(r##"<path d="M32 14l-10-6v12zM32 14l10-6v12z" fill="#{c}"/><rect x="29" y="11" width="6" height="6" rx="2" fill="#{c}"/>"##),
            _ => String::new(),
        };
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64"><rect x="8" y="14" width="48" height="44" rx="6" fill="#{h}"/><rect x="8" y="48" width="48" height="10" rx="0" fill="#000" opacity="0.08"/>{eyes}<rect x="27" y="40" width="10" height="3" rx="1.5" fill="#1d2430" opacity="0.55"/>{top}</svg>"##,
            h = self.head
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatars_round_trip_and_refuse_odd_values() {
        let a = Avatar::from_choices("3d8fd6", "happy", "cap", "e0663c").unwrap();
        assert_eq!(Avatar::parse(&a.encode(), "jo"), a);
        assert!(Avatar::from_choices("ff0000\"><script>", "happy", "cap", "e0663c").is_none());
        let p = Avatar::parse("h=<b>;e=x", "jo");
        assert_eq!(p, Avatar::default_for("jo"));
        assert!(a.svg().starts_with("<svg"));
    }
}
