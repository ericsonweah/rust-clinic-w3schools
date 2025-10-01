



fn main (){

  enum LoginStatus {
    Success(String),
   Error(String),
  }

  let result1 = LoginStatus::Success(String::from("Welcome back"));
  let _result2 = LoginStatus::Error(String::from("Incorrect password"));  


  match result1 {
    LoginStatus::Success(msg) => println!("{}", msg),
    LoginStatus::Error(msg) => println!("{}", msg),
  }
}