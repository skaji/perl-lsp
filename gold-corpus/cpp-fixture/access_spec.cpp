/* hitlist-2 #18: member completion filters by access specifier. */
class Status {
 public:
  bool ok() const;
  void Update(int x) {
    this->Ref();
    this->
  }

 private:
  void Ref();
  void Unref();
  int rep_;
};

void external_use(Status& status) {
  status.
}
