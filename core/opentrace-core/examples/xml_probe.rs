use opentrace_core::security::expat_message;
fn main() {
    let cases = ["", "   ", "\n\n  ", "junk", "<a>", "<a>\n<b>\n", "<a></b>", "<a><b></a>", "<a>x</a>y", "<a/><b/>", "<a x=\"1\" x=\"2\"/>", "<p:a/>", "<a>&foo;</a>", "<a x=1/>", "<a", "<a x=\"1", "<a>&</a>", "<?xml version=\"1.0\"?>", "<a><</a>", "<!-- c -->", "<a>\u{1}</a>", "</a>", "<a></a><!-- x -->", "<1a/>", "<a b/>", "<a>]]></a>", "<a x=\"<\"/>", "<a>\n  <b>\n  </c>\n</a>", "<a/>trailing", "  <a/> x"];
    for c in cases {
        match roxmltree::Document::parse(c) {
            Ok(_) => println!("{:?} OK", c),
            Err(e) => println!("{:?} -> {}", c, expat_message(c, &e)),
        }
    }
}
