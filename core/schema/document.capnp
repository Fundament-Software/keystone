@0x96830a19d58cde1a;

enum FieldType {
  primary @0;
  revision @1;
  index @2;
}

annotation fieldtype(field) :FieldType;