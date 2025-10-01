

use std::collections::HashMap;

fn main (){
  
  let mut capital_cities = HashMap::new();
  capital_cities.insert("France", "Paris");
  capital_cities.insert("Germany", "Berlin");
  println!("{:?}", capital_cities);
}