pub fn normalize_decimal(s: &str) -> String {
    match s.parse::<f64>() {
        Ok(num) => {
            // Format without trailing zeros
            let formatted = format!("{}", num);
            // Handle the case where we get scientific notation for very small numbers
            if formatted.contains('e') {
                s.to_string() // fallback to original if scientific notation
            } else {
                formatted
            }
        }
        Err(_) => s.to_string(), // fallback to original if not a valid number
    }
}